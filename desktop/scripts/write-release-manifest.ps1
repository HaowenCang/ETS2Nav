#Requires -Version 5.1
<#
.SYNOPSIS
  写出发布外层清单与校验文件（P4R 发布工程 §18）。

.DESCRIPTION
  在 ZIP 旁生成三件东西：

    SHA256SUMS.txt         `<sha256>  <filename>`（两个空格，与 sha256sum 约定一致）
    release-manifest.json  外层发布清单
    release-notes-rc.md    候选版本发布说明草稿（默认写到 desktop/scripts/）

  外层清单的意义在于**它不转述 bundle 对自己的说法**。bundle-manifest.json 是产物内部的自述，
  任何能改写产物的人都能顺手改写它；本脚本因此对**解压后的真实字节**重新求值：

    * `bundle_tree_sha256` = 解压根 `ETS2Nav/` 的完整树摘要（BundleCommon.ps1 的 Get-TreeDigest）
    * 逐项复核 bundle-manifest.json 声明的 desktop/sidecar 的 sha256+bytes、web 与 dataset 的
      files+bytes+tree_sha256；任何一项与实际不符即判 FAIL（退出码 1），而不是照抄
    * 插件 DLL 的身份取自解压树的插件目录，与 bundle 的自述无关

  取值来源同样不写死：
    * `maplibre_version` 读 tools/ets2nav-web/package.json 的 dependencies["maplibre-gl"]
    * `scs_sdk_sha256`  读 telemetry-plugin/scs-sdk-provenance.json 的 sdk.sha256
    * `dataset.archive_sha256` 读 scripts/ci/prepare-dataset.ps1 的 $Sha256 默认值（这是仓库里
      唯一记录数据集归档摘要的地方，docs/validation/p4r-batch5-2026-09.md 亦记有同一摘要）；
      若本机存在 ets2nav-dataset-europe-v5.zip，则一并核对文件哈希；核对不上即 FAIL。
    * `code_signing` 由 Get-AuthenticodeSignature 对四个二进制实测得出，不按假设填写。

  -Check 模式重新计算并与已写出的 release-manifest.json 逐键比对（generated_at 因依赖
  -SourceDateEpoch 而单独打印、不参与判定），不一致即退出码 1；同时要求 validation_report
  指向的文件确实存在。

  编码：本文件含中文，必须以 UTF-8 **带 BOM** 保存。

.PARAMETER ZipPath
  发布 ZIP 路径。

.PARAMETER ExtractDir
  ZIP 的解压根（其中含 `ETS2Nav/`）。

.PARAMETER OutDir
  SHA256SUMS.txt 与 release-manifest.json 的输出目录；缺省为 ZIP 所在目录。

.PARAMETER Check
  只校验不写入。已写出的清单与产物不一致时退出码 1。

.PARAMETER ValidationReport
  仓库相对路径，指向记录本次验证结果的报告。写模式只记录该路径；-Check 模式要求它存在。

.PARAMETER ReleaseNotesPath
  发布说明草稿输出路径；缺省 desktop/scripts/release-notes-rc.md。

.PARAMETER SourceDateEpoch
  Unix 秒，决定 generated_at（默认 1767225600 = 2026-01-01T00:00:00Z）。确定性取值使 -Check
  在没有额外参数时也能复现同一清单。

.EXITCODES
  0 成功（写模式：已写出且自校验通过；-Check：一致）；1 不一致或 FAIL；3 前置条件；4 harness 失败。
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ZipPath,
    [Parameter(Mandatory)][string]$ExtractDir,
    [string]$RepoRoot,
    [string]$OutDir,
    [string]$ValidationReport,
    [string]$ReleaseNotesPath,
    [int64]$SourceDateEpoch = 1767225600,
    [string]$DatasetReleaseTag = 'dataset-europe-v5',
    [switch]$Check
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

if (-not $RepoRoot) { $RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot) }
. (Join-Path $PSScriptRoot 'BundleCommon.ps1')

$script:Failures = New-Object System.Collections.ArrayList
$script:Checks = 0

function Stop-Precondition {
    param([Parameter(Mandatory)][string]$Message)
    Write-Host "PRECONDITION FAILURE: $Message"
    exit 3
}
function Check {
    param([string]$Name, [bool]$Ok, [string]$Detail = '')
    $script:Checks++
    if ($Ok) {
        Write-Host "  [PASS] $Name$(if ($Detail) { " - $Detail" })"
    } else {
        [void]$script:Failures.Add($Name)
        Write-Host "  [FAIL] $Name$(if ($Detail) { " - $Detail" })"
    }
}
function Get-Sha256Local {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Get-RelPath {
    param([string]$Root, [string]$Full)
    return $Full.Substring($Root.Length).TrimStart('\', '/') -replace '\\', '/'
}

# 取 JSON 值的子键名。必须同时处理 ConvertFrom-Json 产生的 PSCustomObject 与
# 本脚本构造的 OrderedDictionary——只认 .PSObject.Properties 会把字典自身的
# Count/Keys/Values 当成内容，从而静默地「比较通过」。
function Get-JsonChildNames {
    param($V)
    if ($null -eq $V) { return @() }
    if ($V -is [string]) { return @() }
    if ($V -is [System.Collections.IDictionary]) {
        return @($V.Keys | ForEach-Object { [string]$_ })
    }
    if ($V -is [System.Collections.IEnumerable]) { return @() }
    return @($V.PSObject.Properties | Where-Object { $_.MemberType -eq 'NoteProperty' -or $_.MemberType -eq 'Property' } | ForEach-Object { $_.Name })
}
function Get-JsonChildValue {
    param($V, [string]$Name)
    if ($V -is [System.Collections.IDictionary]) { return $V[$Name] }
    return $V.$Name
}
function Get-JsonScalar {
    param($V)
    return (ConvertTo-Json -InputObject $V -Depth 12 -Compress)
}

# 递归比较两个 JSON 值，返回差异描述列表。generated_at 由调用方跳过。
function Compare-JsonValue {
    param($Committed, $Recomputed, [string]$Path = '$')
    $diffs = New-Object System.Collections.ArrayList
    $aNames = @(Get-JsonChildNames -V $Committed)
    $bNames = @(Get-JsonChildNames -V $Recomputed)
    if ($aNames.Count -eq 0 -and $bNames.Count -eq 0) {
        $aj = Get-JsonScalar -V $Committed
        $bj = Get-JsonScalar -V $Recomputed
        if ($aj -ne $bj) { [void]$diffs.Add("$Path : 已提交=$aj 重算=$bj") }
        return , $diffs
    }
    foreach ($n in ($aNames + $bNames | Select-Object -Unique)) {
        if ($aNames -notcontains $n) { [void]$diffs.Add("$Path.$n : 仅存在于重算结果"); continue }
        if ($bNames -notcontains $n) { [void]$diffs.Add("$Path.$n : 仅存在于已提交清单"); continue }
        foreach ($d in (Compare-JsonValue -Committed (Get-JsonChildValue -V $Committed -Name $n) -Recomputed (Get-JsonChildValue -V $Recomputed -Name $n) -Path "$Path.$n")) {
            [void]$diffs.Add($d)
        }
    }
    return , $diffs
}

# ── 前置条件 ─────────────────────────────────────────────────────────────────
if (-not (Test-Path -LiteralPath $ZipPath -PathType Leaf)) { Stop-Precondition "ZIP 不存在: $ZipPath" }
$zipFull = (Resolve-Path -LiteralPath $ZipPath).Path
if (-not (Test-Path -LiteralPath $ExtractDir -PathType Container)) { Stop-Precondition "解压根不存在: $ExtractDir" }
$extractFull = (Resolve-Path -LiteralPath $ExtractDir).Path
$bundleRoot = Join-Path $extractFull 'ETS2Nav'
if (-not (Test-Path -LiteralPath $bundleRoot -PathType Container)) {
    Stop-Precondition "解压根下缺少产物目录 ETS2Nav/: $extractFull"
}
$bmPath = Join-Path $bundleRoot 'bundle-manifest.json'
if (-not (Test-Path -LiteralPath $bmPath -PathType Leaf)) { Stop-Precondition "解压树缺少 bundle-manifest.json" }
$bm = Get-Content -LiteralPath $bmPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($bm.schema -ne 2) { Stop-Precondition "bundle-manifest.json schema 必须是 2（实际 $($bm.schema)）" }
if (-not $OutDir) { $OutDir = Split-Path -Parent $zipFull }
if (-not $ReleaseNotesPath) { $ReleaseNotesPath = Join-Path $OutDir 'release-notes-rc.md' }

Write-Host '=== 发布外层清单（§18）==='
Write-Host "zip      : $zipFull"
Write-Host "extract  : $extractFull"
Write-Host "bundle   : $bundleRoot"
Write-Host "模式     : $(if ($Check) { 'CHECK（只校验）' } else { 'WRITE' })"
Write-Host ''

# ── 产物身份 ─────────────────────────────────────────────────────────────────
$artifactFilename = Split-Path -Leaf $zipFull
$artifactSha = Get-Sha256Local -Path $zipFull
$artifactBytes = [int64](Get-Item -LiteralPath $zipFull).Length
$version = [string]$bm.app_version
$profile = [string]$bm.profile

Write-Host '-- 产物身份 --'
Write-Host "  artifact : $artifactFilename"
Write-Host "  sha256   : $artifactSha"
Write-Host "  bytes    : $artifactBytes"

# ── bundle 自述 vs 解压出的真实字节 ──────────────────────────────────────────
Write-Host ''
Write-Host '-- 解压树复算（不转述 bundle 的自述）--'
$binaryIds = [ordered]@{}
foreach ($pair in @(@('desktop', 'desktop_exe', 'ets2nav-desktop.exe'), @('sidecar', 'sidecar', 'nav-core-cli.exe'))) {
    $label = $pair[0]; $key = $pair[1]; $name = $pair[2]
    $p = Join-Path $bundleRoot $name
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) {
        Check "$label 存在" $false $name
        continue
    }
    $id = Get-FileIdentity -Path $p
    Check "$label sha256 与 bundle 自述一致" ($id.Sha256 -eq [string]$bm.$key.sha256) "actual=$($id.Sha256.Substring(0,16))... declared=$(([string]$bm.$key.sha256).Substring(0,16))..."
    Check "$label bytes 与 bundle 自述一致" ($id.Bytes -eq [int64]$bm.$key.bytes) "actual=$($id.Bytes) declared=$($bm.$key.bytes)"
    $binaryIds[$label] = $id
}

$webDir = Join-Path $bundleRoot ($bm.web.dir -replace '/', '\')
if (-not (Test-Path -LiteralPath $webDir -PathType Container)) {
    Check 'web 目录存在' $false $webDir
} else {
    $we = Get-TreeEntries -Root $webDir
    $wt = Get-TreeTotals -Entries $we
    Check 'web files 与 bundle 自述一致' ($wt.Files -eq [int64]$bm.web.files) "actual=$($wt.Files) declared=$($bm.web.files)"
    Check 'web bytes 与 bundle 自述一致' ($wt.Bytes -eq [int64]$bm.web.bytes) "actual=$($wt.Bytes) declared=$($bm.web.bytes)"
    $wd = Get-TreeDigest -Entries $we
    Check 'web tree_sha256 与 bundle 自述一致' ($wd -eq [string]$bm.web.tree_sha256) "actual=$($wd.Substring(0,16))... declared=$(([string]$bm.web.tree_sha256).Substring(0,16))..."
}

$datasetDir = Join-Path $bundleRoot ($bm.dataset.dir -replace '/', '\')
$datasetTotals = $null
$datasetDigest = $null
if (-not (Test-Path -LiteralPath $datasetDir -PathType Container)) {
    Check 'dataset 目录存在' $false $datasetDir
} else {
    $de = Get-TreeEntries -Root $datasetDir
    $datasetTotals = Get-TreeTotals -Entries $de
    $datasetDigest = Get-TreeDigest -Entries $de
    Check 'dataset files 与 bundle 自述一致' ($datasetTotals.Files -eq [int64]$bm.dataset.files) "actual=$($datasetTotals.Files) declared=$($bm.dataset.files)"
    Check 'dataset bytes 与 bundle 自述一致' ($datasetTotals.Bytes -eq [int64]$bm.dataset.bytes) "actual=$($datasetTotals.Bytes) declared=$($bm.dataset.bytes)"
    Check 'dataset tree_sha256 与 bundle 自述一致' ($datasetDigest -eq [string]$bm.dataset.tree_sha256) "actual=$($datasetDigest.Substring(0,16))... declared=$(([string]$bm.dataset.tree_sha256).Substring(0,16))..."
}

$bundleEntries = Get-TreeEntries -Root $bundleRoot
$bundleTotals = Get-TreeTotals -Entries $bundleEntries
$bundleTree = Get-TreeDigest -Entries $bundleEntries
Write-Host "  bundle_tree_sha256 = $bundleTree（$($bundleTotals.Files) 个文件，$($bundleTotals.Bytes) B）"

# 数据集自述（取自解压树，而不是本机 data/ 目录）
$dsManifestPath = Join-Path $datasetDir 'manifest.json'
$dsManifest = $null
if (Test-Path -LiteralPath $dsManifestPath -PathType Leaf) {
    $dsManifest = Get-Content -LiteralPath $dsManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
}

# ── 可选资源 ─────────────────────────────────────────────────────────────────
Write-Host ''
Write-Host '-- 可选资源（basemap / fonts）--'
# 注意：当前 assemble-bundle.ps1 在资源缺席时把 provenance 写成空字符串而不是 null
# （其 [string]$BasemapProvenance 参数对同名变量施加了字符串类型约束，$null 被强制转成 ''）。
# 这里把 null 与空字符串都视为「缺席」，并在外层清单中统一记为 null。
$basemapPresent = [bool]$bm.basemap.present
$mapPmtilesSha = $null
if ($basemapPresent) {
    $bp = Join-Path $bundleRoot ($bm.basemap.path -replace '/', '\')
    if (-not (Test-Path -LiteralPath $bp -PathType Leaf)) {
        Check 'basemap present=true 时文件存在' $false $bp
    } else {
        $bid = Get-FileIdentity -Path $bp
        Check 'basemap sha256 与 bundle 自述一致' ($bid.Sha256 -eq [string]$bm.basemap.sha256)
        Check 'basemap bytes 与 bundle 自述一致' ($bid.Bytes -eq [int64]$bm.basemap.bytes)
        $mapPmtilesSha = $bid.Sha256
    }
}
$fontsTreeSha = $null
if ([bool]$bm.fonts.present) {
    $fp = Join-Path $bundleRoot ($bm.fonts.dir -replace '/', '\')
    if (-not (Test-Path -LiteralPath $fp -PathType Container)) {
        Check 'fonts present=true 时目录存在' $false $fp
    } else {
        $fe = Get-TreeEntries -Root $fp
        $ftt = Get-TreeTotals -Entries $fe
        $fd = Get-TreeDigest -Entries $fe
        Check 'fonts files 与 bundle 自述一致' ($ftt.Files -eq [int64]$bm.fonts.files)
        Check 'fonts bytes 与 bundle 自述一致' ($ftt.Bytes -eq [int64]$bm.fonts.bytes)
        Check 'fonts tree_sha256 与 bundle 自述一致' ($fd -eq [string]$bm.fonts.tree_sha256)
        $fontsTreeSha = $fd
    }
}
Check 'profile 与资源存在性一致' (
    ([string]$bm.profile -eq 'FULL') -eq ($basemapPresent -and [bool]$bm.fonts.present)
) "profile=$($bm.profile) basemap=$basemapPresent fonts=$([bool]$bm.fonts.present)"

# ── 插件 DLL ─────────────────────────────────────────────────────────────────
Write-Host ''
Write-Host '-- 遥测插件 --'
$pluginsDir = Join-Path $bundleRoot 'plugins'
$pluginList = New-Object System.Collections.ArrayList
if (-not (Test-Path -LiteralPath $pluginsDir -PathType Container)) {
    Check 'plugins 目录存在' $false $pluginsDir
} else {
    $dlls = @(Get-ChildItem -LiteralPath $pluginsDir -File | Where-Object { $_.Extension -eq '.dll' } | Sort-Object Name)
    Check '插件 DLL 数量为 2' ($dlls.Count -eq 2) "actual=$($dlls.Count)"
    foreach ($d in $dlls) {
        $id = Get-FileIdentity -Path $d.FullName
        [void]$pluginList.Add([ordered]@{ name = $d.Name; sha256 = $id.Sha256; bytes = $id.Bytes })
        Write-Host "  $($d.Name)  $($id.Bytes) B  $($id.Sha256)"
    }
}

# ── 代码签名（实测，不假设）──────────────────────────────────────────────────
Write-Host ''
Write-Host '-- 代码签名（Get-AuthenticodeSignature 实测）--'
$signedFiles = New-Object System.Collections.ArrayList
$signTargets = @(
    [pscustomobject]@{ Name = 'ets2nav-desktop.exe';   Path = (Join-Path $bundleRoot 'ets2nav-desktop.exe') },
    [pscustomobject]@{ Name = 'nav-core-cli.exe';      Path = (Join-Path $bundleRoot 'nav-core-cli.exe') }
)
if (Test-Path -LiteralPath $pluginsDir -PathType Container) {
    foreach ($d in (Get-ChildItem -LiteralPath $pluginsDir -File | Where-Object { $_.Extension -eq '.dll' } | Sort-Object Name)) {
        $signTargets += [pscustomobject]@{ Name = $d.Name; Path = $d.FullName }
    }
}
$anySigned = $false
foreach ($t in $signTargets) {
    if (-not (Test-Path -LiteralPath $t.Path -PathType Leaf)) {
        [void]$signedFiles.Add([ordered]@{
            name = $t.Name; path = (Get-RelPath -Root $extractFull -Full $t.Path)
            status = 'FILE-MISSING'; status_message = '文件不存在'; signer = $null; timestamped = $false
        })
        continue
    }
    $status = 'UNKNOWN'; $statusMessage = ''; $signer = $null; $timestamped = $false
    try {
        $sig = Get-AuthenticodeSignature -LiteralPath $t.Path
        $status = [string]$sig.Status
        # StatusMessage 带的是本机绝对路径：外层清单同样不得记录调用机器的文件系统位置。
        $statusMessage = ([string]$sig.StatusMessage).Replace($t.Path, (Get-RelPath -Root $extractFull -Full $t.Path)).Replace($extractFull, '<extract>')
        if ($null -ne $sig.SignerCertificate) { $signer = [string]$sig.SignerCertificate.Subject }
        $timestamped = ($null -ne $sig.TimeStamperCertificate)
        if ($status -eq 'Valid') { $anySigned = $true }
    } catch {
        $status = 'ERROR'
        $statusMessage = $_.Exception.Message.Replace($extractFull, '<extract>')
    }
    [void]$signedFiles.Add([ordered]@{
        name = $t.Name; path = (Get-RelPath -Root $extractFull -Full $t.Path)
        status = $status; status_message = $statusMessage; signer = $signer; timestamped = $timestamped
    })
    Write-Host "  $($t.Name): $status$(if ($statusMessage) { " - $statusMessage" })"
}
$codeSigning = [ordered]@{
    verdict    = $(if ($anySigned) { 'SIGNED' } else { 'UNSIGNED' })
    any_signed = $anySigned
    files      = @($signedFiles)
}

# ── 取值来源（不写死）────────────────────────────────────────────────────────
Write-Host ''
Write-Host '-- 外部取值来源 --'
$webPkgPath = Join-Path $RepoRoot 'tools/ets2nav-web/package.json'
if (-not (Test-Path -LiteralPath $webPkgPath -PathType Leaf)) { Stop-Precondition "缺少 $webPkgPath" }
$webPkg = Get-Content -LiteralPath $webPkgPath -Raw -Encoding UTF8 | ConvertFrom-Json
$maplibreVersion = [string]$webPkg.dependencies.'maplibre-gl'
if (-not $maplibreVersion) { Stop-Precondition "tools/ets2nav-web/package.json 的 dependencies['maplibre-gl'] 为空" }
Write-Host "  maplibre-gl        : $maplibreVersion（读自 tools/ets2nav-web/package.json）"

$sdkProvPath = Join-Path $RepoRoot 'telemetry-plugin/scs-sdk-provenance.json'
if (-not (Test-Path -LiteralPath $sdkProvPath -PathType Leaf)) { Stop-Precondition "缺少 $sdkProvPath" }
$sdkProv = Get-Content -LiteralPath $sdkProvPath -Raw -Encoding UTF8 | ConvertFrom-Json
$scsSdkSha = [string]$sdkProv.sdk.sha256
if ($scsSdkSha -notmatch '^[0-9a-fA-F]{64}$') { Stop-Precondition "scs-sdk-provenance.json 的 sdk.sha256 非法: $scsSdkSha" }
Write-Host "  scs sdk sha256     : $scsSdkSha（读自 telemetry-plugin/scs-sdk-provenance.json）"

# 数据集归档摘要：仓库内唯一记录点在 scripts/ci/prepare-dataset.ps1 的 $Sha256 默认值
$prepareDataset = Join-Path $RepoRoot 'scripts/ci/prepare-dataset.ps1'
$datasetArchiveSha = $null
$datasetArchiveShaSource = $null
if (Test-Path -LiteralPath $prepareDataset -PathType Leaf) {
    $scriptText = Get-Content -LiteralPath $prepareDataset -Raw -Encoding UTF8
    $m = [regex]::Match($scriptText, "\`$Sha256\s*=\s*'([0-9a-fA-F]{64})'")
    if ($m.Success) {
        $datasetArchiveSha = $m.Groups[1].Value.ToLowerInvariant()
        $datasetArchiveShaSource = 'scripts/ci/prepare-dataset.ps1'
    }
}
if (-not $datasetArchiveSha) {
    Write-Host '  dataset archive    : 仓库内没有记录数据集归档摘要 → 记为 null（不臆造取值）'
} else {
    Write-Host "  dataset archive    : $datasetArchiveSha（读自 $datasetArchiveShaSource）"
    $localArchive = Join-Path $RepoRoot 'ets2nav-dataset-europe-v5.zip'
    if (Test-Path -LiteralPath $localArchive -PathType Leaf) {
        $actual = Get-Sha256Local -Path $localArchive
        Check '本机数据集归档的 sha256 与仓库记录的摘要一致' ($actual -eq $datasetArchiveSha) "actual=$($actual.Substring(0,16))... recorded=$($datasetArchiveSha.Substring(0,16))..."
    } else {
        Write-Host '  （本机无 ets2nav-dataset-europe-v5.zip，未做文件级核对）'
    }
}

# ── 组装外层清单 ─────────────────────────────────────────────────────────────
$realGameValidation = 'REAL-GAME B1-B6 NOT YET VERIFIED：本候选版本未在装有 Euro Truck Simulator 2 的机器上执行 B1–B6 游戏内验证；B1 插件加载、B2 遥测通道、B3 实时匹配、B4 路线与限速、B5 信号灯相位、B6 长时会话均未运行。任何声称游戏内可用性的结论都不由本发布件支持。'

$manifest = [ordered]@{
    schema               = 1
    product              = 'ETS2Nav'
    version              = $version
    profile              = $profile
    source_commit        = [string]$bm.source.commit
    source_dirty         = [bool]$bm.source.dirty
    cargo_profile        = [string]$bm.source.cargo_profile
    target_triple        = [string]$bm.source.target_triple
    artifact_filename    = $artifactFilename
    artifact_bytes       = $artifactBytes
    artifact_sha256      = $artifactSha
    bundle_tree_sha256   = $bundleTree
    bundle_files         = [int64]$bundleTotals.Files
    bundle_bytes         = [int64]$bundleTotals.Bytes
    desktop_sha256       = $(if ($binaryIds.Contains('desktop')) { $binaryIds['desktop'].Sha256 } else { $null })
    sidecar_sha256       = $(if ($binaryIds.Contains('sidecar')) { $binaryIds['sidecar'].Sha256 } else { $null })
    web_tree_sha256      = [string]$bm.web.tree_sha256
    plugins              = @($pluginList)
    dataset              = [ordered]@{
        release_tag      = $DatasetReleaseTag
        archive_sha256   = $datasetArchiveSha
        archive_sha256_source = $datasetArchiveShaSource
        tree_sha256      = $datasetDigest
        files            = $(if ($null -ne $datasetTotals) { [int64]$datasetTotals.Files } else { $null })
        bytes            = $(if ($null -ne $datasetTotals) { [int64]$datasetTotals.Bytes } else { $null })
        dataset_version  = $(if ($null -ne $dsManifest) { $dsManifest.dataset_version } else { $null })
        game_version     = $(if ($null -ne $dsManifest) { [string]$dsManifest.game_version } else { $null })
        content_fingerprint = $(if ($null -ne $dsManifest) { [string]$dsManifest.content_fingerprint } else { $null })
        generated_at     = $(if ($null -ne $dsManifest) { [string]$dsManifest.generated_at } else { $null })
    }
    map_pmtiles_sha256   = $mapPmtilesSha
    fonts_tree_sha256    = $fontsTreeSha
    maplibre_version     = $maplibreVersion
    scs_sdk_sha256       = $scsSdkSha
    validation_report    = $ValidationReport
    real_game_validation = $realGameValidation
    code_signing         = $codeSigning
    generated_at         = [DateTimeOffset]::FromUnixTimeSeconds($SourceDateEpoch).UtcDateTime.ToString('yyyy-MM-ddTHH:mm:ssZ')
}

# ── 校验模式 ─────────────────────────────────────────────────────────────────
if ($Check) {
    Write-Host ''
    Write-Host '-- 与已提交清单比对 ----------------------------------------------'
    $committedPath = Join-Path $OutDir 'release-manifest.json'
    if (-not (Test-Path -LiteralPath $committedPath -PathType Leaf)) {
        Stop-Precondition "已提交的 release-manifest.json 不存在: $committedPath"
    }
    $committed = Get-Content -LiteralPath $committedPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $diffs = New-Object System.Collections.ArrayList
    $committedProps = @($committed.PSObject.Properties | ForEach-Object { $_.Name })
    $recomputedProps = @($manifest.Keys)
    foreach ($k in $recomputedProps) {
        if ($k -eq 'generated_at') { continue }
        if ($committedProps -notcontains $k) { [void]$diffs.Add("$k : 已提交清单缺少该键"); continue }
        foreach ($d in (Compare-JsonValue -Committed $committed.$k -Recomputed $manifest[$k] -Path $k)) { [void]$diffs.Add($d) }
    }
    foreach ($k in $committedProps) {
        if ($recomputedProps -notcontains $k) { [void]$diffs.Add("$k : 仅存在于已提交清单（重算未产生该键）") }
    }
    Write-Host ("  generated_at（不参与判定）: 已提交=$($committed.generated_at) 重算=$($manifest['generated_at'])")
    foreach ($d in $diffs) { [void]$script:Failures.Add($d); Write-Host "  [FAIL] $d" }
    if ($diffs.Count -eq 0) { Write-Host '  [PASS] 已提交清单与产物重算结果逐键一致' }

    $reportPath = if (-not $ValidationReport) { Join-Path $OutDir 'release-validation-report.md' } elseif ([IO.Path]::IsPathRooted($ValidationReport)) { $ValidationReport } else { Join-Path $RepoRoot ($ValidationReport -replace '/', '\') }
    Check 'validation_report 指向的报告存在' (Test-Path -LiteralPath $reportPath -PathType Leaf) $ValidationReport

    Write-Host ''
    if ($script:Failures.Count -gt 0) {
        Write-Host ("RELEASE MANIFEST CHECK: FAIL ({0})" -f $script:Failures.Count)
        exit 1
    }
    Write-Host 'RELEASE MANIFEST CHECK: PASS'
    exit 0
}

# ── 写出 ─────────────────────────────────────────────────────────────────────
if ($script:Failures.Count -gt 0) {
    Write-Host ''
    Write-Host ("RELEASE MANIFEST: FAIL ({0}) - 解压树与 bundle 自述不一致，拒绝写出外层清单" -f $script:Failures.Count)
    exit 1
}

if (-not (Test-Path -LiteralPath $OutDir)) { $null = New-Item -ItemType Directory -Path $OutDir -Force }
$sumsPath = Join-Path $OutDir 'SHA256SUMS.txt'
$manifestPath = Join-Path $OutDir 'release-manifest.json'
[System.IO.File]::WriteAllText($sumsPath, "$artifactSha  $artifactFilename`n", (New-Object System.Text.UTF8Encoding($false)))
Write-JsonNoBom -Path $manifestPath -Object $manifest

# ── 发布说明草稿 ─────────────────────────────────────────────────────────────
$notesDir = Split-Path -Parent $ReleaseNotesPath
if ($notesDir -and -not (Test-Path -LiteralPath $notesDir)) { $null = New-Item -ItemType Directory -Path $notesDir -Force }
$dsVersion = 'n/a'; $dsGame = 'n/a'; $dsFp = 'n/a'
if ($null -ne $dsManifest) {
    $dsVersion = [string]$dsManifest.dataset_version
    $dsGame = [string]$dsManifest.game_version
    $dsFp = [string]$dsManifest.content_fingerprint
}
$notes = @"
# ETS2Nav $version 候选版本发布说明（草稿）

本文件由 desktop/scripts/write-release-manifest.ps1 从**实测产物**生成，未经人工修饰。
它描述的是一次候选（release candidate）构建，不是最终发布公告。

## 产物

| 项 | 值 |
| --- | --- |
| 文件名 | ``$artifactFilename`` |
| 字节数 | $artifactBytes |
| SHA-256 | ``$artifactSha`` |
| 版本 | $version |
| 组件档位 | $profile |
| 源码提交 | ``$([string]$bm.source.commit)``（工作区有未提交改动: $([bool]$bm.source.dirty)） |
| 解压树摘要 | ``$bundleTree``（``ETS2Nav/`` 全树，$($bundleTotals.Files) 个文件） |

## 数据集来源

| 项 | 值 |
| --- | --- |
| 数据集版本 | $dsVersion |
| 游戏版本 | $dsGame |
| 内容指纹 | ``$dsFp`` |
| 数据集树摘要 | ``$datasetDigest`` |
| 归档摘要 | $(if ($datasetArchiveSha) { "``$datasetArchiveSha``（来源：$datasetArchiveShaSource）" } else { '未在仓库中记录，故为 null' }) |

## 必须如实说明的事项

1. 本发布件**不包含原版游戏归档**。数据集是派生数据（路网几何、路口与检索索引），
   由本项目的编译器从本地安装生成，发布件中不含任何 `*.scs` 归档或游戏资源本体。
2. 运行本发布件**不需要游戏文件**。导航程序、数据集与前端均可独立运行；
   只有实时遥测（车辆位置、速度、信号灯相位）需要游戏在运行且插件已安装。
3. 在本地重建数据集**需要一份合法的游戏安装**。编译数据集的下游步骤要读取游戏归档，
   因此重建能力属于持有该游戏的用户，而不是本发布件所提供的功能。
4. **REAL-GAME B1–B6 NOT YET VERIFIED**。B1 插件加载、B2 遥测通道、B3 实时匹配、
   B4 路线与限速、B5 信号灯相位、B6 长时会话均未在装有游戏的机器上运行过。
   本候选版本的验证全部来自离线装置：单元/集成测试、打包产物校验、隐私扫描与
   无窗口生命周期验证。
5. 本次为 **CORE** 组件档位：底图 ``web/map.pmtiles`` 与字形 ``web/vendor/fonts`` 均**缺席**。
   前端在两者缺席时按既有降级路径运行（无底图 / 跳过城市文字层），
   因此本发布件不得被描述为「完整离线导航界面」。

## 验证入口

外层清单：``release-manifest.json``；校验和：``SHA256SUMS.txt``；
验证报告：``$ValidationReport``（仓库相对路径）。
"@
if ($notes -match '(?i)\b(stable|production-ready|latest)\b') {
    Write-Host 'RELEASE NOTES: FAIL（草稿含被禁用的措辞：stable / production-ready / latest）'
    exit 1
}
[System.IO.File]::WriteAllText($ReleaseNotesPath, $notes, (New-Object System.Text.UTF8Encoding($false)))

# 写出后自校验：SHA256SUMS.txt 必须指向真实字节。
$sumLine = (Get-Content -LiteralPath $sumsPath -Raw -Encoding UTF8).Trim()
$sumOk = ($sumLine -eq "$artifactSha  $artifactFilename")
Check 'SHA256SUMS.txt 记录与产物一致' $sumOk $sumLine
$notesForbidden = @()
foreach ($w in @('stable', 'production-ready', 'latest')) {
    if ($notes -match ('(?i)\b' + [regex]::Escape($w) + '\b')) { $notesForbidden += $w }
}
Check '发布说明不含被禁用措辞' ($notesForbidden.Count -eq 0) ($notesForbidden -join ', ')

Write-Host ''
Write-Host '=== 外层发布层已写出 ==='
Write-Host "SHA256SUMS.txt        : $sumsPath"
Write-Host "release-manifest.json : $manifestPath"
Write-Host "release-notes-rc.md   : $ReleaseNotesPath"
Write-Host "bundle_tree_sha256    : $bundleTree"
Write-Host "code_signing          : $($codeSigning.verdict)"
Write-Host "RELEASE_MANIFEST=$manifestPath"
Write-Host "RELEASE_NOTES=$ReleaseNotesPath"
Write-Host "BUNDLE_TREE_SHA256=$bundleTree"
Write-Host "ARTIFACT_SHA256=$artifactSha"
if ($script:Failures.Count -gt 0) {
    Write-Host ("RELEASE MANIFEST: FAIL ({0})" -f $script:Failures.Count)
    exit 1
}
Write-Host ("RELEASE MANIFEST: PASS（{0} 项复核）" -f $script:Checks)
exit 0
