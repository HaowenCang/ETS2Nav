#Requires -Version 5.1
<#
.SYNOPSIS
  RC 发布流水线：两次独立打包 + A/B 可复现性判定（P4R 发布工程 §16、§17、§31）。

.DESCRIPTION
  对 A、B 两份**互不相干**的打包各跑一遍完整链路，再比较两次结果：

    A/B 各自：
      1. 全新暂存目录 → assemble-bundle.ps1（同一 -SourceDateEpoch）
      2. verify-bundle.ps1  ← 暂存目录
      3. scan-release-privacy.ps1 ← 暂存目录
      4. package-release.ps1  → ZIP X（约定名 ETS2Nav-<version>-windows-x64-<profile>.zip）
      5. sha256(ZIP X)
    A：解压 → verify-bundle → 隐私扫描 → desktop-lifecycle → 写外层清单 → release-artifact.mjs
    B：verify-release-artifact.ps1 -Stage all（含解压/校验/扫描/生命周期/归档断言，
       归档断言用 A 写出的外层清单，因此 B 的插件与二进制必须与清单一致）

  两个判定：
    Payload reproducibility  = 两次解压树的完整树摘要是否相同（**硬门**）
    Archive reproducibility  = sha256(ZIP A) 是否等于 sha256(ZIP B)（PASS 或 NOT CLOSED）

  退出码：0 表示「所有阶段退出码为 0」且两个判定都为 PASS；1 表示有阶段失败、或 payload
  不可复现、或归档不可复现（NOT CLOSED 按未达 §17 目标计，因此是 1 而不是 0）；
  3 前置条件；4 harness 失败。判定与退出码都不因「没跑」而放宽。

  已知环境事实（写进报告，不写成通过）：
    * 本环境没有底图与字形资源，因此 profile 只能是 CORE（-ExpectProfile 默认 CORE）；
      工具本身支持 FULL，但 FULL 路径在本轮无法端到端执行。
    * `dirty=true` 只说明打包时工作区有未提交改动，不是打包缺陷；本脚本会以 WARNING
      形式单独打印，并把它记进报告，供最终 RC 在干净树上重跑时对照。

  编码：本文件含中文，必须以 UTF-8 **带 BOM** 保存。

.PARAMETER Dataset
  数据集源目录（解压后的 europe-v5 资产）。

.PARAMETER WebRoot
  前端构建输出，缺省 tools/ets2nav-web/dist。

.PARAMETER WorkDir
  工作目录（E: 盘，容量充足）。A/B 的暂存、ZIP、解压树、外层清单都放在这里。

.PARAMETER AssembleScript
  组装脚本路径，缺省 desktop/scripts/assemble-bundle.ps1。存在的意义是让流水线可用
  替身脚本（desktop/tests/fixtures/ 下的合成组装器）自检，而不必每次都复制 373 MB 数据集。

.PARAMETER SourceDateEpoch
  两次打包共用的固定瞬时（默认 1767225600 = 2026-01-01T00:00:00Z）。

.EXITCODES
  0 全部通过且两个判定 PASS；1 有失败或判定未通过；3 前置条件；4 harness 失败。
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Dataset,
    [string]$WebRoot,
    [string]$WorkDir,
    [string]$RepoRoot,
    [int64]$SourceDateEpoch = 1767225600,
    [switch]$SkipBuild,
    [string]$ExpectProfile = 'CORE',
    [string]$AssembleScript,
    [string]$ValidationReport,
    [string]$ReleaseNotesPath,
    [switch]$KeepStaging
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

if (-not $RepoRoot) { $RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot) }
if (-not $WorkDir) { $WorkDir = Join-Path $env:TEMP 'ets2nav-rc-pipeline' }
if (-not $WebRoot) { $WebRoot = Join-Path $RepoRoot 'tools/ets2nav-web/dist' }
if (-not $AssembleScript) { $AssembleScript = Join-Path $RepoRoot 'desktop/scripts/assemble-bundle.ps1' }
$scriptsDir = Join-Path $RepoRoot 'desktop/scripts'
$testsDir = Join-Path $RepoRoot 'desktop/tests'

. (Join-Path $scriptsDir 'BundleCommon.ps1')

$hostExe = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
# 报告与发布说明是**流水线输出**，默认写在发布目录（与 ZIP 同级），不写进仓库：写进仓库
# 会让打包过程用自己的产物把工作树弄脏，使 dirty=false 永远不可达。
# 必须在 $built['A'] 被填充**之后**才能解析默认值——$built 由 A/B 循环创建。
function Resolve-ReportPath {
    param([string]$Spec, [string]$Default)
    if (-not $Spec) { return $Default }
    if ([IO.Path]::IsPathRooted($Spec)) { return $Spec }
    return (Join-Path $RepoRoot ($Spec -replace '/', '\'))
}
$script:Stages = New-Object System.Collections.ArrayList
$pipelineSw = [System.Diagnostics.Stopwatch]::StartNew()
$inputDrift = @()
$fpBefore = $null
$fpAfter = $null

# 输入快照：A/B 判定只有在「两次打包面对同一组输入」时才有意义。工作树可能在本轮流水线
# 运行期间被其他工作流修改（真实发生过：Cargo.lock 在 A 与 B 之间被重写、
# THIRD_PARTY_NOTICES.txt 在同一窗口内出现三个不同内容），此时 payload 差异来自输入差异，
# 而不是打包不确定性。快照覆盖两侧：便宜且敏感的 git 工作树状态，以及逐项身份
# （脚本、许可、构建清单、两个二进制、插件、前端产物与数据集的树摘要）。
function Get-InputFingerprint {
    param(
        [Parameter(Mandatory)][string]$RepoRootPath,
        [Parameter(Mandatory)][string]$DatasetPath,
        [Parameter(Mandatory)][string]$WebRootPath,
        [Parameter(Mandatory)][string]$AssemblePath,
        [Parameter(Mandatory)][string[]]$SelfOutputs
    )
    $items = [ordered]@{}
    Push-Location $RepoRootPath
    try {
        $status = @(& git status --porcelain) | Where-Object {
            $line = $_
            $keep = $true
            foreach ($self in $SelfOutputs) { if ($line -like "*$self*") { $keep = $false } }
            $keep
        }
        $items['git-worktree-status'] = ($status -join "`n")
    } finally { Pop-Location }
    foreach ($rel in @(
        'desktop/scripts/assemble-bundle.ps1', 'desktop/scripts/verify-bundle.ps1',
        'LICENSE', 'THIRD_PARTY_NOTICES.txt',
        'desktop/Cargo.toml', 'desktop/Cargo.lock', 'nav-core/Cargo.lock',
        'desktop/target/release/ets2nav-desktop.exe', 'nav-core/target/release/nav-core-cli.exe',
        'telemetry-plugin/scs-nav-bridge/out/scs-nav-bridge.dll',
        'telemetry-plugin/semaphore-bridge/out/semaphore-bridge.dll',
        'tools/ets2nav-web/package.json', 'telemetry-plugin/scs-sdk-provenance.json'
    )) {
        $p = Join-Path $RepoRootPath ($rel -replace '/', '\')
        if (Test-Path -LiteralPath $p -PathType Leaf) {
            $items[$rel] = (Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()
        } else {
            $items[$rel] = 'MISSING'
        }
    }
    foreach ($pair in @(
        @('tree:desktop/src', (Join-Path $RepoRootPath 'desktop/src')),
        @('tree:nav-core-cli/src', (Join-Path $RepoRootPath 'nav-core/tools/nav-core-cli/src')),
        @('tree:web-dist', $WebRootPath),
        @('tree:dataset', $DatasetPath)
    )) {
        $label = $pair[0]; $dir = $pair[1]
        if (Test-Path -LiteralPath $dir -PathType Container) {
            $items[$label] = Get-TreeDigest -Entries (Get-TreeEntries -Root $dir)
        } else {
            $items[$label] = 'MISSING'
        }
    }
    return $items
}

function Compare-InputFingerprint {
    param([Parameter(Mandatory)]$Before, [Parameter(Mandatory)]$After)
    $changed = New-Object System.Collections.ArrayList
    foreach ($k in $Before.Keys) {
        if (-not $After.Contains($k)) { [void]$changed.Add("$k : 快照后消失"); continue }
        if ($Before[$k] -ne $After[$k]) { [void]$changed.Add($k) }
    }
    foreach ($k in $After.Keys) { if (-not $Before.Contains($k)) { [void]$changed.Add("$k : 快照后新出现") } }
    return , $changed
}

function Format-FingerprintValue {
    param([Parameter(Mandatory)][string]$Value)
    if ($Value.Length -le 20) { return $Value }
    return $Value.Substring(0, 20) + '…'
}

function Write-Banner {
    param([string]$Text)
    Write-Host ''
    Write-Host ('#' * 78)
    Write-Host "# $Text"
    Write-Host ('#' * 78)
}

function Invoke-Step {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string[]]$Arguments
    )
    Write-Host ''
    Write-Host "=== STAGE $Name"
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $out = & $FilePath @Arguments 2>&1
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prev
        $sw.Stop()
    }
    foreach ($line in @($out)) { Write-Host ("    | " + [string]$line) }
    if ($null -eq $code) { $code = 0 }
    $code = [int]$code
    [void]$script:Stages.Add([pscustomobject]@{
        Name = $Name; Code = $code; Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1)
    })
    Write-Host ("STAGE_EXIT name={0} code={1} seconds={2}" -f $Name, $code, [math]::Round($sw.Elapsed.TotalSeconds, 1))
    return $code
}

function Get-ManifestOf {
    param([Parameter(Mandatory)][string]$StagingDir)
    $p = Join-Path $StagingDir 'bundle-manifest.json'
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { return $null }
    return (Get-Content -LiteralPath $p -Raw -Encoding UTF8 | ConvertFrom-Json)
}

# 逐文件比较两棵树，给出差异清单（可复现性失败时必须能定位到具体文件，而不是只说「不同」）。
function Compare-TreePair {
    param([Parameter(Mandatory)][string]$RootA, [Parameter(Mandatory)][string]$RootB)
    $ea = Get-TreeEntries -Root $RootA
    $eb = Get-TreeEntries -Root $RootB
    $mapA = @{}; foreach ($e in $ea) { $mapA[$e.Rel] = $e }
    $mapB = @{}; foreach ($e in $eb) { $mapB[$e.Rel] = $e }
    $onlyA = @($mapA.Keys | Where-Object { -not $mapB.ContainsKey($_) } | Sort-Object)
    $onlyB = @($mapB.Keys | Where-Object { -not $mapA.ContainsKey($_) } | Sort-Object)
    $differing = New-Object System.Collections.ArrayList
    foreach ($k in @($mapA.Keys | Where-Object { $mapB.ContainsKey($_) } | Sort-Object)) {
        if ($mapA[$k].Sha256 -ne $mapB[$k].Sha256) {
            [void]$differing.Add([pscustomobject]@{
                Rel = $k
                ShaA = $mapA[$k].Sha256
                ShaB = $mapB[$k].Sha256
                BytesA = $mapA[$k].Bytes
                BytesB = $mapB[$k].Bytes
            })
        }
    }
    return [pscustomobject]@{
        OnlyA = $onlyA; OnlyB = $onlyB; Differing = @($differing)
        Identical = ($onlyA.Count -eq 0 -and $onlyB.Count -eq 0 -and $differing.Count -eq 0)
    }
}

try {
    Write-Banner 'RC 发布流水线：A/B 两次独立打包与可复现性判定'
    Write-Host "repo        : $RepoRoot"
    Write-Host "dataset     : $Dataset"
    Write-Host "webroot     : $WebRoot"
    Write-Host "workdir     : $WorkDir"
    Write-Host "assemble    : $AssembleScript"
    Write-Host "epoch       : $SourceDateEpoch ($([DateTimeOffset]::FromUnixTimeSeconds($SourceDateEpoch).UtcDateTime.ToString('yyyy-MM-ddTHH:mm:ssZ')))"
    Write-Host "expect prof : $ExpectProfile"
    Write-Host "skip build  : $([bool]$SkipBuild)"

    # ── 前置条件 ─────────────────────────────────────────────────────────────
    foreach ($p in @($AssembleScript, (Join-Path $scriptsDir 'verify-bundle.ps1'),
                     (Join-Path $scriptsDir 'scan-release-privacy.ps1'),
                     (Join-Path $scriptsDir 'package-release.ps1'),
                     (Join-Path $scriptsDir 'write-release-manifest.ps1'),
                     (Join-Path $scriptsDir 'verify-release-artifact.ps1'),
                     (Join-Path $testsDir 'desktop-lifecycle.mjs'),
                     (Join-Path $testsDir 'release-artifact.mjs'))) {
        if (-not (Test-Path -LiteralPath $p -PathType Leaf)) {
            Write-Host "PRECONDITION FAILURE: 缺少必要文件: $p"; exit 3
        }
    }
    if (-not (Test-Path -LiteralPath $Dataset -PathType Container)) {
        Write-Host "PRECONDITION FAILURE: 数据集目录不存在: $Dataset"; exit 3
    }
    if (-not (Test-Path -LiteralPath $WebRoot -PathType Container)) {
        Write-Host "PRECONDITION FAILURE: 前端产物目录不存在: $WebRoot"; exit 3
    }
    # 许可与第三方声明是发布件的一部分：缺失即前置条件失败，不由本脚本生成。
    foreach ($f in @('LICENSE', 'THIRD_PARTY_NOTICES.txt')) {
        $p = Join-Path $RepoRoot $f
        if (-not (Test-Path -LiteralPath $p -PathType Leaf)) {
            Write-Host "PRECONDITION FAILURE: 仓库根缺少 $f（发布件必须随包分发许可与第三方声明）"; exit 3
        }
    }
    # 组装脚本接口自检：缺 -SourceDateEpoch 说明面对的是 schema 1 的旧实现，A/B 无从谈起。
    $assembleParams = @((Get-Command -Name $AssembleScript).Parameters.Keys)
    foreach ($need in @('OutDir', 'Dataset', 'WebRoot', 'Profile', 'SkipBuild', 'MapPmtiles', 'BasemapProvenance', 'FontsDir', 'FontsProvenance', 'SourceDateEpoch')) {
        if ($assembleParams -notcontains $need) {
            Write-Host "PRECONDITION FAILURE: $AssembleScript 缺少参数 -$need（发布工程约定的接口未就位）"; exit 3
        }
    }

    $assembleSha = (Get-FileHash -LiteralPath $AssembleScript -Algorithm SHA256).Hash.ToLowerInvariant()
    $verifyBundleSha = (Get-FileHash -LiteralPath (Join-Path $scriptsDir 'verify-bundle.ps1') -Algorithm SHA256).Hash.ToLowerInvariant()
    Push-Location $RepoRoot
    try {
        $gitCommit = (& git rev-parse HEAD).Trim()
        $gitDirty = @(& git status --porcelain).Count -gt 0
        $gitChangeCount = @(& git status --porcelain).Count
    } finally { Pop-Location }
    Write-Host "git         : commit=$gitCommit dirty=$gitDirty changes=$gitChangeCount"
    Write-Host "assemble sha: $assembleSha"
    Write-Host "verify  sha : $verifyBundleSha"

    if ($gitDirty) {
        Write-Host ''
        Write-Host 'WARNING: 工作区不干净（dirty=true）。这不是打包缺陷：本轮打包的两份产物都出自同一棵工作树，'
        Write-Host '         A/B 比较仍然有效。但最终 RC 必须在干净树上重跑，届时 manifest 必须显示 dirty=false。'
    }

    Write-Host ''
    Write-Host '== 输入快照（打包前）'
    $selfOutputs = @('release-validation-report.md', 'release-notes-rc.md')
    $fpBefore = Get-InputFingerprint -RepoRootPath $RepoRoot -DatasetPath $Dataset -WebRootPath $WebRoot `
        -AssemblePath $AssembleScript -SelfOutputs $selfOutputs
    foreach ($k in $fpBefore.Keys) { Write-Host ("   {0,-52} {1}" -f $k, (Format-FingerprintValue -Value ([string]$fpBefore[$k]))) }
    if (Test-Path -LiteralPath $WorkDir) { Remove-Item -LiteralPath $WorkDir -Recurse -Force }
    $null = New-Item -ItemType Directory -Path $WorkDir -Force

    # ── A / B：组装 → 校验 → 扫描 → 打包 ─────────────────────────────────────
    $built = [ordered]@{}
    foreach ($tag in @('A', 'B')) {
        Write-Banner "[$tag] 组装 → 校验 → 隐私扫描 → 打包"
        $dir = Join-Path $WorkDir $tag
        $null = New-Item -ItemType Directory -Path $dir -Force
        $staging = Join-Path $dir 'staging'
        $assembleArgs = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $AssembleScript,
                          '-OutDir', $staging, '-Dataset', $Dataset, '-WebRoot', $WebRoot,
                          '-Profile', 'release', '-SourceDateEpoch', "$SourceDateEpoch")
        if ($SkipBuild) { $assembleArgs += '-SkipBuild' }
        $rcAssemble = Invoke-Step -Name "$tag/assemble-bundle" -FilePath $hostExe -Arguments $assembleArgs

        $bm = Get-ManifestOf -StagingDir $staging
        if ($null -eq $bm) {
            Write-Host "FAIL: [$tag] 组装未产出 bundle-manifest.json"
            [void]$script:Stages.Add([pscustomobject]@{ Name = "$tag/manifest-read"; Code = 1; Seconds = 0 })
            continue
        }
        if ($bm.schema -ne 2) {
            Write-Host "FAIL: [$tag] bundle-manifest.json schema=$($bm.schema)，发布工程要求 schema 2"
            [void]$script:Stages.Add([pscustomobject]@{ Name = "$tag/manifest-schema"; Code = 1; Seconds = 0 })
        }
        if ([bool]$bm.source.dirty) {
            Write-Host "WARNING: [$tag] bundle-manifest.json 记录 dirty=true（工作区未提交改动；见上文说明）"
        }
        $profileLower = ([string]$bm.profile).ToLowerInvariant()
        $artifactName = "ETS2Nav-$($bm.app_version)-windows-x64-$profileLower.zip"
        $zipPath = Join-Path $dir $artifactName

        $null = Invoke-Step -Name "$tag/verify-bundle(staging)" -FilePath $hostExe `
            -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $scriptsDir 'verify-bundle.ps1'), '-BundleDir', $staging)
        $null = Invoke-Step -Name "$tag/privacy-scan(staging)" -FilePath $hostExe `
            -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $scriptsDir 'scan-release-privacy.ps1'), '-Target', $staging)
        $null = Invoke-Step -Name "$tag/package-release" -FilePath $hostExe `
            -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $scriptsDir 'package-release.ps1'),
                         '-StagingDir', $staging, '-ZipPath', $zipPath, '-SourceDateEpoch', "$SourceDateEpoch")

        $zipInfo = $null
        if (Test-Path -LiteralPath $zipPath -PathType Leaf) {
            $zipInfo = Get-FileIdentity -Path $zipPath
        } else {
            Write-Host "FAIL: [$tag] 未产出 $artifactName"
        }
        $built[$tag] = [pscustomobject]@{
            Tag = $tag; Dir = $dir; Staging = $staging; ZipPath = $zipPath
            ArtifactName = $artifactName; Zip = $zipInfo; Manifest = $bm
            Profile = [string]$bm.profile; Version = [string]$bm.app_version
            RcAssemble = $rcAssemble
            ExtractRoot = (Join-Path $dir 'extract')
            BundleRoot = (Join-Path $dir 'extract\ETS2Nav')
        }
        if (-not $KeepStaging) {
            Write-Host "== 清理暂存目录（ZIP 与解压树保留）：$staging"
            Remove-Item -LiteralPath $staging -Recurse -Force
        }
    }

    # $built 现已填充：在此解析两个输出路径的默认值（此处是它们的最早可用点）。
    if (-not $ReleaseNotesPath) { $ReleaseNotesPath = Join-Path $built['A'].Dir 'release-notes-rc.md' }
    if (-not $reportPath) { $reportPath = Join-Path $built['A'].Dir 'release-validation-report.md' }
    # 清单里记录的是**相对指针**（与 ZIP 同级），不是任何机器的绝对路径。
    $validationPointer = if ($ValidationReport) { $ValidationReport } else { 'release-validation-report.md' }

    if ($null -eq $built['A'].Zip -or $null -eq $built['B'].Zip) {
        Write-Host ''
        Write-Host 'PIPELINE: FAIL（至少一份产物未生成，A/B 比较无法进行）'
        exit 1
    }

    # ── A：解压 → 逐阶段验证 → 外层清单 → 归档断言 ───────────────────────────
    Write-Banner '[A] 解压与验证（逐阶段记录真实退出码）'
    $null = Invoke-Step -Name 'A/verify-release-artifact(extract)' -FilePath $hostExe `
        -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $scriptsDir 'verify-release-artifact.ps1'),
                     '-ZipPath', $built['A'].ZipPath, '-ExtractDir', $built['A'].ExtractRoot, '-Stage', 'extract')
    $null = Invoke-Step -Name 'A/verify-bundle(extracted)' -FilePath $hostExe `
        -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $scriptsDir 'verify-bundle.ps1'), '-BundleDir', $built['A'].BundleRoot)
    $null = Invoke-Step -Name 'A/privacy-scan(extracted)' -FilePath $hostExe `
        -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $scriptsDir 'scan-release-privacy.ps1'), '-Target', $built['A'].BundleRoot)
    $null = Invoke-Step -Name 'A/desktop-lifecycle' -FilePath 'node' `
        -Arguments @((Join-Path $testsDir 'desktop-lifecycle.mjs'), '--bundle', $built['A'].BundleRoot)

    Write-Banner '[A] 写出外层发布层（§18）'
    $null = Invoke-Step -Name 'A/write-release-manifest' -FilePath $hostExe `
        -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $scriptsDir 'write-release-manifest.ps1'),
                     '-ZipPath', $built['A'].ZipPath, '-ExtractDir', $built['A'].ExtractRoot,
                     '-OutDir', $built['A'].Dir, '-ValidationReport', $validationPointer,
                     '-ReleaseNotesPath', $ReleaseNotesPath, '-SourceDateEpoch', "$SourceDateEpoch")
    $releaseManifestPath = Join-Path $built['A'].Dir 'release-manifest.json'
    $null = Invoke-Step -Name 'A/release-artifact.mjs' -FilePath 'node' `
        -Arguments @((Join-Path $testsDir 'release-artifact.mjs'), '--zip', $built['A'].ZipPath,
                     '--extract', $built['A'].ExtractRoot, '--release-manifest', $releaseManifestPath,
                     '--expect-profile', $ExpectProfile)

    # ── B：整链（§4 的包装器在本轮被真正端到端执行）──────────────────────────
    Write-Banner '[B] verify-release-artifact.ps1 -Stage all（§16 全链，归档断言用 A 的外层清单）'
    $null = Invoke-Step -Name 'B/verify-release-artifact(all)' -FilePath $hostExe `
        -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $scriptsDir 'verify-release-artifact.ps1'),
                     '-ZipPath', $built['B'].ZipPath, '-ExtractDir', $built['B'].ExtractRoot,
                     '-ReleaseManifest', $releaseManifestPath, '-ExpectProfile', $ExpectProfile)

    Write-Host ''
    Write-Host '== 输入快照（打包后）'
    $fpAfter = Get-InputFingerprint -RepoRootPath $RepoRoot -DatasetPath $Dataset -WebRootPath $WebRoot `
        -AssemblePath $AssembleScript -SelfOutputs @('release-validation-report.md', 'release-notes-rc.md')
    $inputDrift = Compare-InputFingerprint -Before $fpBefore -After $fpAfter
    if ($inputDrift.Count -eq 0) {
        Write-Host '   INPUT FREEZE: STABLE（打包期间输入未变，A/B 判定有效）'
    } else {
        Write-Host '   INPUT FREEZE: DRIFT DETECTED（打包期间输入发生变化，A/B 判定无效）'
        foreach ($k in $inputDrift) {
            Write-Host ("     变化: {0}" -f $k)
            Write-Host ("       前: {0}" -f (Format-FingerprintValue -Value ([string]$fpBefore[$k])))
            Write-Host ("       后: {0}" -f (Format-FingerprintValue -Value ([string]$fpAfter[$k])))
        }
    }
    # ── 比较 ─────────────────────────────────────────────────────────────────
    Write-Banner 'A/B 可复现性判定'
    $payload = 'FAIL'
    $archive = 'NOT CLOSED'
    $treeA = $null; $treeB = $null; $compare = $null
    if ((Test-Path -LiteralPath $built['A'].BundleRoot -PathType Container) -and (Test-Path -LiteralPath $built['B'].BundleRoot -PathType Container)) {
        $entriesA = Get-TreeEntries -Root $built['A'].BundleRoot
        $entriesB = Get-TreeEntries -Root $built['B'].BundleRoot
        $treeA = Get-TreeDigest -Entries $entriesA
        $treeB = Get-TreeDigest -Entries $entriesB
        $compare = Compare-TreePair -RootA $built['A'].BundleRoot -RootB $built['B'].BundleRoot
        if ($treeA -eq $treeB -and $compare.Identical) { $payload = 'PASS' }
    } else {
        Write-Host 'FAIL: 至少一棵解压树不存在，payload 判定无法进行（记为 FAIL，不记为未执行）'
    }
    if ($built['A'].Zip.Sha256 -eq $built['B'].Zip.Sha256) { $archive = 'PASS' }

    Write-Host "tree digest A : $treeA"
    Write-Host "tree digest B : $treeB"
    Write-Host "zip sha A     : $($built['A'].Zip.Sha256)"
    Write-Host "zip sha B     : $($built['B'].Zip.Sha256)"
    if ($null -ne $compare -and -not $compare.Identical) {
        Write-Host '-- 差异定位 --'
        foreach ($r in $compare.OnlyA) { Write-Host "  仅存在于 A: $r" }
        foreach ($r in $compare.OnlyB) { Write-Host "  仅存在于 B: $r" }
        foreach ($d in $compare.Differing) {
            Write-Host ("  内容不同  : {0}`n              A {1} ({2} B)`n              B {3} ({4} B)" -f $d.Rel, $d.ShaA, $d.BytesA, $d.ShaB, $d.BytesB)
        }
    }

    $pipelineSw.Stop()
    $totalSeconds = [math]::Round($pipelineSw.Elapsed.TotalSeconds, 1)
    $failedStages = @($script:Stages | Where-Object { $_.Code -ne 0 })

    # ── 验证报告 ─────────────────────────────────────────────────────────────

    $reportDir = Split-Path -Parent $reportPath
    if (-not (Test-Path -LiteralPath $reportDir)) { $null = New-Item -ItemType Directory -Path $reportDir -Force }
    $sb = New-Object System.Text.StringBuilder
    [void]$sb.AppendLine('# ETS2Nav RC 发布验证报告（A/B 可复现性）')
    [void]$sb.AppendLine()
    [void]$sb.AppendLine('本报告由 `desktop/scripts/run-release-pipeline.ps1` 在真实运行中生成；表中的每个退出码都是子进程实际返回值。')
    [void]$sb.AppendLine()
    [void]$sb.AppendLine('## 运行环境与输入')
    [void]$sb.AppendLine()
    [void]$sb.AppendLine('| 项 | 值 |')
    [void]$sb.AppendLine('| --- | --- |')
    [void]$sb.AppendLine("| 源码提交 | ``$gitCommit``（dirty=$gitDirty，未提交改动 $gitChangeCount 项） |")
    [void]$sb.AppendLine("| assemble-bundle.ps1 | sha256 ``$assembleSha`` |")
    [void]$sb.AppendLine("| verify-bundle.ps1 | sha256 ``$verifyBundleSha`` |")
    [void]$sb.AppendLine("| 数据集源 | ``$Dataset`` |")
    [void]$sb.AppendLine("| 前端源 | ``$WebRoot`` |")
    [void]$sb.AppendLine("| SourceDateEpoch | $SourceDateEpoch |")
    [void]$sb.AppendLine("| 工作目录 | ``$WorkDir`` |")
    [void]$sb.AppendLine("| 期望档位 | $ExpectProfile |")
    [void]$sb.AppendLine("| 总墙钟 | $totalSeconds s |")
    [void]$sb.AppendLine()
    [void]$sb.AppendLine('## 阶段退出码（实测）')
    [void]$sb.AppendLine()
    [void]$sb.AppendLine('| 阶段 | 退出码 | 秒 |')
    [void]$sb.AppendLine('| --- | --- | --- |')
    foreach ($s in $script:Stages) { [void]$sb.AppendLine("| ``$($s.Name)`` | $($s.Code) | $($s.Seconds) |") }
    [void]$sb.AppendLine()
    [void]$sb.AppendLine('## 可复现性判定')
    [void]$sb.AppendLine()
    [void]$sb.AppendLine("    Payload reproducibility = $payload")
    [void]$sb.AppendLine("    Archive reproducibility = $archive")
    [void]$sb.AppendLine()
    [void]$sb.AppendLine("| 项 | A | B |")
    [void]$sb.AppendLine('| --- | --- | --- |')
    [void]$sb.AppendLine("| ZIP 文件名 | ``$($built['A'].ArtifactName)`` | ``$($built['B'].ArtifactName)`` |")
    [void]$sb.AppendLine("| ZIP 字节数 | $($built['A'].Zip.Bytes) | $($built['B'].Zip.Bytes) |")
    [void]$sb.AppendLine("| ZIP sha256 | ``$($built['A'].Zip.Sha256)`` | ``$($built['B'].Zip.Sha256)`` |")
    [void]$sb.AppendLine("| 解压树摘要 | ``$treeA`` | ``$treeB`` |")
    [void]$sb.AppendLine()
    if ($null -ne $compare) {
        if ($compare.Identical) {
            [void]$sb.AppendLine('两次解压树的逐文件身份完全一致（无独有文件、无内容差异）。')
        } else {
            [void]$sb.AppendLine('差异定位：')
            [void]$sb.AppendLine()
            foreach ($r in $compare.OnlyA) { [void]$sb.AppendLine("- 仅存在于 A: ``$r``") }
            foreach ($r in $compare.OnlyB) { [void]$sb.AppendLine("- 仅存在于 B: ``$r``") }
            foreach ($d in $compare.Differing) {
                [void]$sb.AppendLine("- ``$($d.Rel)``：A ``$($d.ShaA)``（$($d.BytesA) B） / B ``$($d.ShaB)``（$($d.BytesB) B）")
            }
        }
    }
    [void]$sb.AppendLine()
    [void]$sb.AppendLine('## 输入冻结（判定有效性的前提）')
    [void]$sb.AppendLine()
    if ($inputDrift.Count -eq 0) {
        [void]$sb.AppendLine('打包前后两次输入快照完全一致：A/B 判定有效。')
    } else {
        [void]$sb.AppendLine('**输入在打包期间发生变化，A/B 判定因此无效**：差异来自输入改动，而不是打包不确定性。')
        [void]$sb.AppendLine()
        foreach ($k in $inputDrift) {
            [void]$sb.AppendLine("- ``$k``：前 ``$(Format-FingerprintValue -Value ([string]$fpBefore[$k]))`` / 后 ``$(Format-FingerprintValue -Value ([string]$fpAfter[$k]))``")
        }
    }
    [void]$sb.AppendLine()
    [void]$sb.AppendLine('## 未验证项（NOT VERIFIED）')
    [void]$sb.AppendLine()
    [void]$sb.AppendLine('1. **REAL-GAME B1–B6 未运行**：本机未安装 Euro Truck Simulator 2，游戏内验证不在本轮范围内。')
    [void]$sb.AppendLine('2. **FULL 档位未端到端执行**：本环境没有底图 ``map.pmtiles`` 与字形 ``fonts/`` 资源，')
    [void]$sb.AppendLine('   因此只可能产出 CORE 产物；FULL 路径（含 provenance 校验、字形 ranges 覆盖检查）本工具支持但本轮未执行。')
    [void]$sb.AppendLine('3. **代码签名**：无证书，``Get-AuthenticodeSignature`` 对四个二进制报告 NotSigned；签名后需重跑本流水线。')
    if ($gitDirty) {
        [void]$sb.AppendLine('4. **干净树**：本轮打包时工作区 dirty=true；最终 RC 必须在干净树上重跑，manifest 应显示 dirty=false。')
    }
    [System.IO.File]::WriteAllText($reportPath, $sb.ToString(), (New-Object System.Text.UTF8Encoding($false)))
    Write-Host ''
    Write-Host "验证报告: $reportPath"

    # ── 最终：从 A 再写一次外层清单，并用 -Check 复算核对 ────────────────────
    Write-Banner '最终：从 A 写出外层清单 + -Check 复算'
    $null = Invoke-Step -Name 'final/write-release-manifest' -FilePath $hostExe `
        -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $scriptsDir 'write-release-manifest.ps1'),
                     '-ZipPath', $built['A'].ZipPath, '-ExtractDir', $built['A'].ExtractRoot,
                     '-OutDir', $built['A'].Dir, '-ValidationReport', $validationPointer,
                     '-ReleaseNotesPath', $ReleaseNotesPath, '-SourceDateEpoch', "$SourceDateEpoch")
    $null = Invoke-Step -Name 'final/release-manifest-check' -FilePath $hostExe `
        -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $scriptsDir 'write-release-manifest.ps1'),
                     '-ZipPath', $built['A'].ZipPath, '-ExtractDir', $built['A'].ExtractRoot,
                     '-OutDir', $built['A'].Dir, '-ValidationReport', $validationPointer,
                     '-Check', '-SourceDateEpoch', "$SourceDateEpoch")

    $failedStages = @($script:Stages | Where-Object { $_.Code -ne 0 })
    Write-Banner '最终判定'
    Write-Host "source commit              = $gitCommit (dirty=$gitDirty)"
    Write-Host "assemble-bundle.ps1 sha256 = $assembleSha"
    Write-Host ''
    if ($inputDrift.Count -eq 0) {
        Write-Host 'Input freeze            = STABLE'
    } else {
        Write-Host ('Input freeze            = DRIFT DETECTED: ' + ($inputDrift -join ', '))
        Write-Host '                          （A/B 判定无效：打包期间输入被改动，差异来自输入而非打包不确定性）'
    }    Write-Host "Payload reproducibility = $payload"
    Write-Host "Archive reproducibility = $archive"
    Write-Host ''
    Write-Host "ZIP A = $($built['A'].ZipPath)"
    Write-Host "        $($built['A'].Zip.Bytes) B  sha256=$($built['A'].Zip.Sha256)"
    Write-Host "ZIP B = $($built['B'].ZipPath)"
    Write-Host "        $($built['B'].Zip.Bytes) B  sha256=$($built['B'].Zip.Sha256)"
    Write-Host "tree digest A = $treeA"
    Write-Host "tree digest B = $treeB"
    Write-Host ''
    Write-Host "总墙钟 = $totalSeconds s"
    Write-Host "阶段数 = $($script:Stages.Count)，失败 $($failedStages.Count)"
    if ($failedStages.Count -gt 0) {
        foreach ($s in $failedStages) { Write-Host ("  失败阶段: {0} → {1}" -f $s.Name, $s.Code) }
    }
    Write-Host "验证报告 = $reportPath"

    $ok = ($failedStages.Count -eq 0) -and ($payload -eq 'PASS') -and ($archive -eq 'PASS')
    if ($ok) {
        Write-Host 'RC RELEASE PIPELINE: PASS'
        exit 0
    }
    Write-Host 'RC RELEASE PIPELINE: FAIL（见上文阶段退出码与判定；NOT CLOSED 按未达 §17 目标计）'
    exit 1
} catch {
    Write-Host ("HARNESS FAILURE: {0}" -f $_.Exception.Message)
    Write-Host $_.ScriptStackTrace
    exit 4
}
