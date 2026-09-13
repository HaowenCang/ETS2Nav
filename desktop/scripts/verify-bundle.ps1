#Requires -Version 5.1
<#
.SYNOPSIS
  Verify an assembled Desktop bundle against its own manifest (P4R Batch 6A section 6).

.DESCRIPTION
  This is the packaging gate. It re-derives every identity recorded by
  `assemble-bundle.ps1` from the bytes actually present in the bundle:

    desktop executable   sha256 + bytes
    sidecar executable   sha256 + bytes
    web/                 file count + total bytes + canonical tree digest
    data/europe-v5/      file count + total bytes + canonical tree digest + required files
    basemap / fonts      presence flag must match what is on disk

  It is deliberately independent of the assembly script: it re-reads the directory
  rather than trusting anything cached in memory, so a bundle that was tampered
  with after assembly fails here. `docs/validation` never claims package integrity
  on the strength of the assembly step alone.

  ASCII only, so no BOM contract is needed.

.PARAMETER BundleDir
  Bundle directory produced by assemble-bundle.ps1.

.PARAMETER SkipDatasetDigest
  Skip re-hashing the dataset tree (373 MB). The count/bytes/required-file checks
  still run. Using this makes the run fast but weakens the gate, so it is reported
  as a weaker mode rather than silently accepted.

.EXITCODES
  0 all checks PASS; 1 FAIL; 3 precondition (bundle or manifest missing).
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$BundleDir,
    [switch]$SkipDatasetDigest
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'BundleCommon.ps1')

$fail = New-Object System.Collections.ArrayList
$pass = 0
function Check {
    param([string]$Name, [bool]$Ok, [string]$Detail = '')
    if ($Ok) {
        $script:pass++
        Write-Host "  [PASS] $Name$(if ($Detail) { " - $Detail" })"
    } else {
        [void]$script:fail.Add($Name)
        Write-Host "  [FAIL] $Name$(if ($Detail) { " - $Detail" })"
    }
}

if (-not (Test-Path -LiteralPath $BundleDir -PathType Container)) {
    Write-Host "PRECONDITION FAILURE: bundle directory not found: $BundleDir"
    exit 3
}
$manifestPath = Join-Path $BundleDir 'bundle-manifest.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    Write-Host "PRECONDITION FAILURE: bundle-manifest.json not found in $BundleDir"
    exit 3
}
$m = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json

Write-Host '=== bundle verification ==='
Write-Host "bundle   : $BundleDir"
Write-Host "version  : $($m.app_version)  profile=$($m.profile)"
Write-Host "source   : commit=$($m.source.commit) dirty=$($m.source.dirty) cargo_profile=$($m.source.cargo_profile)"
Write-Host "basemap  : present=$($m.basemap.present)"
Write-Host "fonts    : present=$($m.fonts.present)"
Write-Host ''

Write-Host '-- executables --'
Check 'manifest schema is 2' ($m.schema -eq 2) "schema=$($m.schema)"
# profile 是**测量值**，必须与磁盘上的资源存在性一致。CORE 被打成 FULL（或反之）会让
# 产物名称与 release note 描述的完整性与实际内容不符——这正是「Core RC 不得冒称
# Full Offline」这条要求可以被机械核查的地方。
$expectedProfile = if ([bool]$m.basemap.present -and [bool]$m.fonts.present) { 'FULL' } else { 'CORE' }
Check 'profile matches resource presence' ($m.profile -eq $expectedProfile) "declared=$($m.profile) expected=$expectedProfile"
foreach ($pair in @(@('desktop_exe', 'ets2nav-desktop.exe'), @('sidecar', 'nav-core-cli.exe'))) {
    $key = $pair[0]
    $declaredName = $m.$key.name
    $path = Join-Path $BundleDir $declaredName
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        Check "$key exists ($declaredName)" $false 'file not found'
        continue
    }
    Check "$key name matches convention" ($declaredName -eq $pair[1]) "declared=$declaredName"
    $id = Get-FileIdentity -Path $path
    Check "$key sha256 matches manifest" ($id.Sha256 -eq $m.$key.sha256) "actual=$($id.Sha256.Substring(0,16))... declared=$($m.$key.sha256.Substring(0,16))..."
    Check "$key bytes match manifest" ($id.Bytes -eq [int64]$m.$key.bytes) "actual=$($id.Bytes) declared=$($m.$key.bytes)"
}

Write-Host ''
Write-Host '-- frontend --'
$webDir = Join-Path $BundleDir $m.web.dir
if (-not (Test-Path -LiteralPath $webDir -PathType Container)) {
    Check 'web directory exists' $false $webDir
} else {
    Check 'web/index.html exists' (Test-Path -LiteralPath (Join-Path $webDir 'index.html') -PathType Leaf)
    $we = Get-TreeEntries -Root $webDir
    $wt = Get-TreeTotals -Entries $we
    Check 'web file count matches manifest' ($wt.Files -eq [int64]$m.web.files) "actual=$($wt.Files) declared=$($m.web.files)"
    Check 'web total bytes matches manifest' ($wt.Bytes -eq [int64]$m.web.bytes) "actual=$($wt.Bytes) declared=$($m.web.bytes)"
    $wd = Get-TreeDigest -Entries $we
    Check 'web tree digest matches manifest' ($wd -eq $m.web.tree_sha256) "actual=$($wd.Substring(0,16))... declared=$($m.web.tree_sha256.Substring(0,16))..."
}

Write-Host ''
Write-Host '-- basemap / glyphs --'
$basemapPath = Join-Path $BundleDir ($m.basemap.path -replace '/', '\')
$basemapOnDisk = Test-Path -LiteralPath $basemapPath -PathType Leaf
Check 'basemap presence flag matches disk' ([bool]$m.basemap.present -eq $basemapOnDisk) "manifest=$($m.basemap.present) disk=$basemapOnDisk"
if ($basemapOnDisk) {
    $bid = Get-FileIdentity -Path $basemapPath
    Check 'basemap sha256 matches manifest' ($bid.Sha256 -eq $m.basemap.sha256)
    Check 'basemap bytes matches manifest' ($bid.Bytes -eq [int64]$m.basemap.bytes)
    Check 'basemap is non-trivial (>1024 B)' ($bid.Bytes -gt 1024) "bytes=$($bid.Bytes)"
}
$fontsDir = Join-Path $BundleDir ($m.fonts.dir -replace '/', '\')
$fontsOnDisk = Test-Path -LiteralPath $fontsDir -PathType Container
Check 'fonts presence flag matches disk' ([bool]$m.fonts.present -eq $fontsOnDisk) "manifest=$($m.fonts.present) disk=$fontsOnDisk"
if ($fontsOnDisk) {
    $fe = Get-TreeEntries -Root $fontsDir
    $ft = Get-TreeTotals -Entries $fe
    Check 'fonts file count matches manifest' ($ft.Files -eq [int64]$m.fonts.files)
    Check 'fonts bytes matches manifest' ($ft.Bytes -eq [int64]$m.fonts.bytes)
    # schema 2 起记录字形树的摘要：只比对文件数会让「换掉内容、保持文件数」通过。
    $fd = Get-TreeDigest -Entries $fe
    Check 'fonts tree digest matches manifest' ($fd -eq $m.fonts.tree_sha256) "actual=$($fd.Substring(0,16))... declared=$($m.fonts.tree_sha256.Substring(0,16))..."
    # ranges 声明必须与磁盘分片互相覆盖（与 assemble-bundle.ps1 同一条判据的独立复核）。
    $prov = $m.fonts.provenance
    if ($null -eq $prov) {
        Check 'fonts provenance present when glyphs ship' $false 'provenance 为 null'
    } else {
        $declared = @($prov.ranges -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
        $onDisk = @()
        foreach ($g in (Get-ChildItem -LiteralPath $fontsDir -Recurse -File -Filter '*.pbf')) {
            $onDisk += ("{0}/{1}" -f (Split-Path -Leaf (Split-Path -Parent $g.FullName)), [IO.Path]::GetFileNameWithoutExtension($g.Name))
        }
        $missing = @($declared | Where-Object { $onDisk -notcontains "$($prov.font)/$_" })
        Check 'every declared range has a glyph file' ($missing.Count -eq 0) "missing=$($missing -join ', ')"
        $undeclared = @($onDisk | Where-Object { $declared -notcontains ($_ -split '/', 2)[1] })
        Check 'no undeclared glyph file ships' ($undeclared.Count -eq 0) "undeclared=$($undeclared -join ', ')"
    }
}

Write-Host ''
Write-Host '-- dataset --'
$datasetDir = Join-Path $BundleDir ($m.dataset.dir -replace '/', '\')
if (-not (Test-Path -LiteralPath $datasetDir -PathType Container)) {
    Check 'dataset directory exists' $false $datasetDir
} else {
    foreach ($req in @('manifest.json', 'routing.graph', 'junction.graph', 'search.db')) {
        $p = Join-Path $datasetDir $req
        $ok = (Test-Path -LiteralPath $p -PathType Leaf) -and ((Get-Item -LiteralPath $p).Length -gt 0)
        Check "dataset required file $req present and non-empty" $ok
    }
    $de = Get-TreeEntries -Root $datasetDir
    $dt = Get-TreeTotals -Entries $de
    Check 'dataset file count matches manifest' ($dt.Files -eq [int64]$m.dataset.files) "actual=$($dt.Files) declared=$($m.dataset.files)"
    Check 'dataset total bytes matches manifest' ($dt.Bytes -eq [int64]$m.dataset.bytes) "actual=$($dt.Bytes) declared=$($m.dataset.bytes)"
    if ($SkipDatasetDigest) {
        Write-Host '  [SKIPPED] dataset tree digest (weak mode requested)'
    } else {
        $dd = Get-TreeDigest -Entries $de
        Check 'dataset tree digest matches manifest' ($dd -eq $m.dataset.tree_sha256) "actual=$($dd.Substring(0,16))... declared=$($m.dataset.tree_sha256.Substring(0,16))..."
    }
}

Write-Host ''
if ($fail.Count -gt 0) {
    Write-Host "BUNDLE VERIFY: FAIL ($($fail.Count)) - $($fail -join '; ')"
    exit 1
}
if ($SkipDatasetDigest) {
    Write-Host "BUNDLE VERIFY: PASS (weak mode: dataset tree digest not recomputed; $pass checks)"
} else {
    Write-Host "BUNDLE VERIFY: PASS ($pass checks)"
}
exit 0
