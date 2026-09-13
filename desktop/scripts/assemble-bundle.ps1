#Requires -Version 5.1
<#
.SYNOPSIS
  Assemble the ETS2Nav Desktop bundle (P4R Batch 6A sections 6, 9, 10).

.DESCRIPTION
  Builds the two runtime binaries, copies the frontend artefacts and the dataset
  next to them, and writes `bundle-manifest.json` recording the identity of every
  runtime entity. The resulting directory IS the shippable product: the Desktop
  resolves its sidecar, frontend and dataset relative to its own executable and
  refuses to start when any of them is missing or does not match the manifest.

  Layout produced:

    <OutDir>/
      ets2nav-desktop.exe          the product entry point
      nav-core-cli.exe             the sidecar served on 127.0.0.1:<runtime port>
      bundle-manifest.json         identity of all of the above
      web/                         frontend artefacts (served by the sidecar)
      web/map.pmtiles              optional basemap (see -MapPmtiles)
      web/fonts/                   optional glyphs  (see -FontsDir)
      data/europe-v5/              navigation dataset

  Nothing here invents resource provenance: the dataset is copied from a path the
  caller supplies, and the basemap/glyphs are recorded as absent unless the caller
  supplies them. A bundle built without them is honestly recorded as incomplete
  rather than silently declared a full offline product.

  ENCODING: UTF-8 **with BOM**. This file carries Chinese comments, and Windows
  PowerShell 5.1 reads a BOM-less .ps1 as ANSI, which mis-decodes them (the same
  contract scripts/harness-encoding-guard.bat enforces for regression.ps1). A
  mis-decode can even swallow a line break and comment out the next statement, so
  do not re-save this file without the BOM.

.PARAMETER OutDir
  Destination bundle directory. Created if absent; existing content is replaced.

.PARAMETER Dataset
  Source dataset directory (the extracted europe-v5 release asset).

.PARAMETER WebRoot
  Frontend build output. Defaults to tools/ets2nav-web/dist.

.PARAMETER Profile
  release (default) or debug. release is the shipping configuration.

.PARAMETER SkipBuild
  Do not run cargo; use whatever binaries already exist.

.PARAMETER MapPmtiles
  Optional basemap archive to place at web/map.pmtiles.

.PARAMETER FontsDir
  Optional glyph directory to place at web/fonts.

.EXITCODES
  0 bundle assembled; 1 FAIL; 3 precondition (missing input, build failed).
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$OutDir,
    [Parameter(Mandatory)][string]$Dataset,
    [string]$WebRoot,
    [ValidateSet('release', 'debug')][string]$Profile = 'release',
    [switch]$SkipBuild,
    [string]$MapPmtiles,
    [string]$FontsDir
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$desktopDir = Join-Path $repoRoot 'desktop'
$navCore = Join-Path $repoRoot 'nav-core'
if (-not $WebRoot) { $WebRoot = Join-Path $repoRoot 'tools/ets2nav-web/dist' }

. (Join-Path $PSScriptRoot 'BundleCommon.ps1')

function Invoke-Step {
    param([string]$What, [scriptblock]$Body)
    Write-Host "== $What"
    & $Body
    if ($LASTEXITCODE -ne 0 -and $null -ne $LASTEXITCODE) {
        Write-Host "PRECONDITION FAILURE: $What exited $LASTEXITCODE"
        exit 3
    }
}

$cargoArgs = @()
if ($Profile -eq 'release') { $cargoArgs += '--release' }

if (-not $SkipBuild) {
    Invoke-Step 'build nav-core-cli (sidecar)' {
        Push-Location $navCore
        try { & cargo build @cargoArgs -p nav-core-cli } finally { Pop-Location }
    }
    Invoke-Step 'build ets2nav-desktop' {
        & cargo build @cargoArgs --manifest-path (Join-Path $desktopDir 'Cargo.toml')
    }
}

$profileDir = if ($Profile -eq 'release') { 'release' } else { 'debug' }
$desktopExeSrc = Join-Path $desktopDir "target/$profileDir/ets2nav-desktop.exe"
$sidecarSrc = Join-Path $navCore "target/$profileDir/nav-core-cli.exe"

$missing = @()
foreach ($p in @($desktopExeSrc, $sidecarSrc, $WebRoot, $Dataset)) {
    if (-not (Test-Path -LiteralPath $p)) { $missing += $p }
}
if ($missing.Count -gt 0) {
    Write-Host 'PRECONDITION FAILURE: missing inputs:'
    $missing | ForEach-Object { Write-Host "  - $_" }
    exit 3
}
foreach ($req in @('manifest.json', 'routing.graph', 'junction.graph', 'search.db')) {
    if (-not (Test-Path -LiteralPath (Join-Path $Dataset $req) -PathType Leaf)) {
        Write-Host "PRECONDITION FAILURE: dataset is missing $req ($Dataset)"
        exit 3
    }
}

if (Test-Path -LiteralPath $OutDir) { Remove-Item -LiteralPath $OutDir -Recurse -Force }
$null = New-Item -ItemType Directory -Path $OutDir -Force

Write-Host "== copy runtime binaries"
Copy-Item -LiteralPath $desktopExeSrc -Destination (Join-Path $OutDir 'ets2nav-desktop.exe') -Force
Copy-Item -LiteralPath $sidecarSrc -Destination (Join-Path $OutDir 'nav-core-cli.exe') -Force

Write-Host "== copy frontend artefacts"
$webOut = Join-Path $OutDir 'web'
Copy-Item -LiteralPath $WebRoot -Destination $webOut -Recurse -Force
# A stray archive in the source directory must never be mistaken for the shipped basemap.
$strayPmtiles = Join-Path $webOut 'map.pmtiles'
if (Test-Path -LiteralPath $strayPmtiles) { Remove-Item -LiteralPath $strayPmtiles -Force }

Write-Host "== copy dataset"
$datasetOut = Join-Path $OutDir 'data/europe-v5'
$null = New-Item -ItemType Directory -Path $datasetOut -Force
# Enumerate with -LiteralPath and copy item by item: `-LiteralPath <dir>\*` would treat
# the wildcard literally and copy nothing, while `-Path` with a wildcard would break on
# file names containing bracket characters.
foreach ($item in (Get-ChildItem -LiteralPath $Dataset -Force)) {
    Copy-Item -LiteralPath $item.FullName -Destination $datasetOut -Recurse -Force
}

# ── optional resources (section 10) ──────────────────────────────────────────
$basemapPresent = $false
$basemapSha = $null
$basemapBytes = 0
if ($MapPmtiles) {
    if (-not (Test-Path -LiteralPath $MapPmtiles -PathType Leaf)) {
        Write-Host "PRECONDITION FAILURE: -MapPmtiles not found: $MapPmtiles"
        exit 3
    }
    Copy-Item -LiteralPath $MapPmtiles -Destination (Join-Path $webOut 'map.pmtiles') -Force
    $id = Get-FileIdentity -Path (Join-Path $webOut 'map.pmtiles')
    $basemapPresent = $true
    $basemapSha = $id.Sha256
    $basemapBytes = $id.Bytes
    Write-Host "   basemap: $($id.Bytes) B sha256=$($id.Sha256)"
} else {
    Write-Host '   basemap: NOT SUPPLIED (recorded as absent in the manifest)'
}
$fontsPresent = $false
$fontsFiles = 0
$fontsBytes = 0
if ($FontsDir) {
    if (-not (Test-Path -LiteralPath $FontsDir)) {
        Write-Host "PRECONDITION FAILURE: -FontsDir not found: $FontsDir"
        exit 3
    }
    # 目标路径必须与前端 style 的 glyphs 模板一致：app.js 用
    # `vendor/fonts/{fontstack}/{range}.pbf`，build.mjs 也把 <web>/fonts 复制到
    # dist/vendor/fonts。放错目录不会报错，只会让文字层静默不出现——因此这里按
    # 服务端实际提供的路径写入，并由 verify-bundle.ps1 用同一路径核对。
    $fontsOut = Join-Path $webOut 'vendor/fonts'
    $null = New-Item -ItemType Directory -Path $fontsOut -Force
    foreach ($item in (Get-ChildItem -LiteralPath $FontsDir -Force)) {
        Copy-Item -LiteralPath $item.FullName -Destination $fontsOut -Recurse -Force
    }
    $fe = Get-TreeEntries -Root $fontsOut
    $ft = Get-TreeTotals -Entries $fe
    $fontsPresent = $true
    $fontsFiles = $ft.Files
    $fontsBytes = $ft.Bytes
    Write-Host "   glyphs : $($ft.Files) files $($ft.Bytes) B -> web/vendor/fonts"
} else {
    Write-Host '   glyphs : NOT SUPPLIED (recorded as absent in the manifest)'
}

# ── identity of everything that will ship ────────────────────────────────────
Write-Host "== hash bundle contents"
$desktopId = Get-FileIdentity -Path (Join-Path $OutDir 'ets2nav-desktop.exe')
$sidecarId = Get-FileIdentity -Path (Join-Path $OutDir 'nav-core-cli.exe')
$webEntries = Get-TreeEntries -Root $webOut
$webTotals = Get-TreeTotals -Entries $webEntries
$webDigest = Get-TreeDigest -Entries $webEntries
$datasetEntries = Get-TreeEntries -Root $datasetOut
$datasetTotals = Get-TreeTotals -Entries $datasetEntries
$datasetDigest = Get-TreeDigest -Entries $datasetEntries

# The dataset carries its own provenance: version, game build and content
# fingerprint written by the map compiler. Record it verbatim rather than
# restating it, so a bundle can be traced back to the exact extraction.
$dsManifest = Get-Content -LiteralPath (Join-Path $datasetOut 'manifest.json') -Raw | ConvertFrom-Json
$source = Get-SourceCommit -RepoRoot $repoRoot

$manifest = [ordered]@{
    schema       = 1
    product      = 'ETS2Nav'
    app_version  = '0.1.0'
    generated_at = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
    source       = [ordered]@{
        commit       = $source.Commit
        dirty        = $source.Dirty
        profile      = $Profile
        target_triple = 'x86_64-pc-windows-msvc'
    }
    desktop_exe  = [ordered]@{
        name   = 'ets2nav-desktop.exe'
        sha256 = $desktopId.Sha256
        bytes  = $desktopId.Bytes
    }
    sidecar      = [ordered]@{
        name   = 'nav-core-cli.exe'
        sha256 = $sidecarId.Sha256
        bytes  = $sidecarId.Bytes
    }
    web          = [ordered]@{
        dir         = 'web'
        files       = $webTotals.Files
        bytes       = $webTotals.Bytes
        tree_sha256 = $webDigest
    }
    dataset      = [ordered]@{
        dir                 = 'data/europe-v5'
        files               = $datasetTotals.Files
        bytes               = $datasetTotals.Bytes
        tree_sha256         = $datasetDigest
        dataset_version     = $dsManifest.dataset_version
        game_version        = $dsManifest.game_version
        content_fingerprint = $dsManifest.content_fingerprint
        generated_at        = $dsManifest.generated_at
    }
    basemap      = [ordered]@{
        path    = 'web/map.pmtiles'
        present = $basemapPresent
        sha256  = $basemapSha
        bytes   = $basemapBytes
        source  = if ($basemapPresent) { $MapPmtiles } else { $null }
    }
    fonts        = [ordered]@{
        dir     = 'web/vendor/fonts'
        present = $fontsPresent
        files   = $fontsFiles
        bytes   = $fontsBytes
        source  = if ($fontsPresent) { $FontsDir } else { $null }
    }
}

Write-JsonNoBom -Path (Join-Path $OutDir 'bundle-manifest.json') -Object $manifest

Write-Host ''
Write-Host '=== bundle assembled ==='
Write-Host "dir       : $OutDir"
Write-Host "profile   : $Profile  commit=$($source.Commit) dirty=$($source.Dirty)"
Write-Host "desktop   : $($desktopId.Bytes) B sha256=$($desktopId.Sha256)"
Write-Host "sidecar   : $($sidecarId.Bytes) B sha256=$($sidecarId.Sha256)"
Write-Host "web       : $($webTotals.Files) files $($webTotals.Bytes) B tree=$($webDigest.Substring(0,16))..."
Write-Host "dataset   : $($datasetTotals.Files) files $($datasetTotals.Bytes) B tree=$($datasetDigest.Substring(0,16))..."
Write-Host "basemap   : present=$basemapPresent"
Write-Host "fonts     : present=$fontsPresent"
exit 0
