#Requires -Version 5.1
<#
.SYNOPSIS
  对「将要上传的字节」做端到端验证（P4R 发布工程 §16）。

.DESCRIPTION
  被判定的对象是 ZIP 归档本身，而不是暂存目录：先把归档解压到一个全新的空路径，再对
  解压出的产物树依次执行四件事。

    extract              解压到全新路径（目标已存在且非空即前置条件失败，避免与旧内容混淆）
    verify-bundle.ps1    产物自述与真实字节的一致性（仓库既有实现，本脚本不重复实现）
    scan-release-privacy 隐私扫描（§5 / §29）
    desktop-lifecycle    运行时生命周期：sidecar 身份、数据集、WS、路线请求、停机、无孤儿
    release-artifact.mjs 归档层断言（顶层布局、禁止内容、档位自洽、插件身份、自述对账）

  每一阶段都打印机器可读的 `STAGE_EXIT name=<阶段> code=<退出码>` 行，供上层脚本汇总；
  退出码为 0 当且仅当**所有阶段都真的执行过且都返回 0**。缺少 `-ReleaseManifest` 时
  归档层断言无法执行，此时整体退出码是 3 而不是 0——「没跑」不等于「通过」。

  `-Stage extract` 只做解压，供编排脚本先取得解压根、再自行逐步调用各阶段并各自记录退出码。

  编码：本文件含中文，必须以 UTF-8 **带 BOM** 保存。

.PARAMETER ZipPath
  待验证的发布 ZIP。

.PARAMETER ExtractDir
  解压目标。必须不存在或为空目录。

.PARAMETER ReleaseManifest
  外层发布清单（release-manifest.json）。归档层断言的插件身份核对依赖它；缺省则该项不执行。

.PARAMETER ExpectProfile
  本次环境期望的档位，默认 CORE（本环境无底图与字形资源）。

.PARAMETER Stage
  all（默认）或 extract（只解压）。

.PARAMETER SkipLifecycle
  跳过运行时生命周期阶段。跳过会被明确记为「未执行」，不会让整体变成通过。

.EXITCODES
  0 所有阶段执行且通过；1 有阶段失败；3 前置条件或必要阶段未执行；4 harness 失败。
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ZipPath,
    [Parameter(Mandatory)][string]$ExtractDir,
    [string]$ReleaseManifest,
    [string]$ExpectProfile = 'CORE',
    [ValidateSet('all', 'extract')][string]$Stage = 'all',
    [string]$RepoRoot,
    [switch]$SkipLifecycle
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

Add-Type -AssemblyName System.IO.Compression | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem | Out-Null

if (-not $RepoRoot) { $RepoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot) }
$scriptsDir = Join-Path $RepoRoot 'desktop/scripts'
$testsDir = Join-Path $RepoRoot 'desktop/tests'

function Stop-Precondition {
    param([Parameter(Mandatory)][string]$Message)
    Write-Host "PRECONDITION FAILURE: $Message"
    exit 3
}

# 子进程执行：不能用 `& script.ps1` 就地调用——那些脚本以 exit N 结束，会连带终止本进程。
$hostExe = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
$script:Stages = New-Object System.Collections.ArrayList

function Invoke-Stage {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(Mandatory)][string[]]$Arguments
    )
    Write-Host ''
    Write-Host "=== STAGE $Name ==="
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $out = & $FilePath @Arguments 2>&1
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prev
    }
    foreach ($line in @($out)) { Write-Host ("    | " + [string]$line) }
    if ($null -eq $code) { $code = 0 }
    $code = [int]$code
    [void]$script:Stages.Add([pscustomobject]@{ Name = $Name; Code = $code })
    Write-Host ("STAGE_EXIT name={0} code={1}" -f $Name, $code)
    return $code
}

try {
    Write-Host '=== 发布产物端到端验证（§16）==='
    Write-Host "zip    : $ZipPath"
    Write-Host "extract: $ExtractDir"
    Write-Host "stage  : $Stage"

    if (-not (Test-Path -LiteralPath $ZipPath -PathType Leaf)) { Stop-Precondition "ZIP 不存在: $ZipPath" }
    $zipFull = (Resolve-Path -LiteralPath $ZipPath).Path
    if ((Get-Item -LiteralPath $zipFull).Length -le 0) { Stop-Precondition "ZIP 是空文件: $zipFull" }

    # ── 解压到全新路径 ────────────────────────────────────────────────────────
    if (Test-Path -LiteralPath $ExtractDir) {
        $existing = @(Get-ChildItem -LiteralPath $ExtractDir -Force)
        if ($existing.Count -gt 0) {
            Stop-Precondition "解压目标已存在且非空: $ExtractDir（必须是全新路径，避免与上一次的产物混淆）"
        }
    } else {
        $null = New-Item -ItemType Directory -Path $ExtractDir -Force
    }
    $extractFull = (Resolve-Path -LiteralPath $ExtractDir).Path
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    [System.IO.Compression.ZipFile]::ExtractToDirectory($zipFull, $extractFull)
    $sw.Stop()
    $entries = @(Get-ChildItem -LiteralPath $extractFull -Force)
    Write-Host ''
    Write-Host ("== extracted {0} 项到 {1}（{2} s）" -f $entries.Count, $extractFull, [math]::Round($sw.Elapsed.TotalSeconds, 1))
    foreach ($e in $entries) { Write-Host ("   - " + $e.Name) }
    $bundleRoot = Join-Path $extractFull 'ETS2Nav'
    if (-not (Test-Path -LiteralPath $bundleRoot -PathType Container)) {
        Write-Host "FAIL: 解压根下没有 ETS2Nav/（归档顶层布局不符合发布约定）"
        exit 1
    }

    if ($Stage -eq 'extract') {
        Write-Host ''
        Write-Host "EXTRACTED_ROOT=$extractFull"
        Write-Host "EXTRACTED_BUNDLE=$bundleRoot"
        Write-Host 'VERIFY ARTIFACT: EXTRACT OK'
        exit 0
    }

    # ── 各阶段 ───────────────────────────────────────────────────────────────
    $bundleVerify = Join-Path $scriptsDir 'verify-bundle.ps1'
    $privacyScan = Join-Path $scriptsDir 'scan-release-privacy.ps1'
    $lifecycle = Join-Path $testsDir 'desktop-lifecycle.mjs'
    $artifactAssert = Join-Path $testsDir 'release-artifact.mjs'
    foreach ($p in @($bundleVerify, $privacyScan, $lifecycle, $artifactAssert)) {
        if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { Stop-Precondition "缺少必要脚本: $p" }
    }

    $codes = [ordered]@{}
    $codes['verify-bundle'] = Invoke-Stage -Name 'verify-bundle' -FilePath $hostExe `
        -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $bundleVerify, '-BundleDir', $bundleRoot)
    $codes['privacy-scan'] = Invoke-Stage -Name 'privacy-scan' -FilePath $hostExe `
        -Arguments @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $privacyScan, '-Target', $bundleRoot)

    if ($SkipLifecycle) {
        Write-Host ''
        Write-Host 'STAGE_SKIPPED name=lifecycle reason=SkipLifecycle'
    } else {
        $codes['lifecycle'] = Invoke-Stage -Name 'lifecycle' -FilePath 'node' `
            -Arguments @($lifecycle, '--bundle', $bundleRoot)
    }

    if (-not $ReleaseManifest) {
        Write-Host ''
        Write-Host 'STAGE_NOT_RUN name=release-artifact reason=未提供 -ReleaseManifest（插件身份核对依赖外层清单，不能跳过）'
    } else {
        if (-not (Test-Path -LiteralPath $ReleaseManifest -PathType Leaf)) {
            Stop-Precondition "release-manifest.json 不存在: $ReleaseManifest"
        }
        $codes['release-artifact'] = Invoke-Stage -Name 'release-artifact' -FilePath 'node' `
            -Arguments @($artifactAssert, '--zip', $zipFull, '--extract', $extractFull,
                         '--release-manifest', (Resolve-Path -LiteralPath $ReleaseManifest).Path,
                         '--expect-profile', $ExpectProfile)
    }

    Write-Host ''
    Write-Host '=== 阶段退出码汇总 ==='
    foreach ($k in $codes.Keys) { Write-Host ("  {0,-18} {1}" -f $k, $codes[$k]) }
    Write-Host "EXTRACTED_BUNDLE=$bundleRoot"

    if (-not $ReleaseManifest) {
        Write-Host 'VERIFY ARTIFACT: PRECONDITION（归档层断言未执行：缺少 -ReleaseManifest）'
        exit 3
    }
    $failed = @($codes.Keys | Where-Object { $codes[$_] -ne 0 })
    if ($failed.Count -gt 0) {
        Write-Host ("VERIFY ARTIFACT: FAIL - 阶段失败: " + (($failed | ForEach-Object { "$_=$($codes[$_])" }) -join ', '))
        exit 1
    }
    Write-Host ("VERIFY ARTIFACT: PASS（{0} 个阶段全部执行且通过）" -f $codes.Keys.Count)
    exit 0
} catch {
    Write-Host ("HARNESS FAILURE: {0}" -f $_.Exception.Message)
    Write-Host $_.ScriptStackTrace
    exit 4
}
