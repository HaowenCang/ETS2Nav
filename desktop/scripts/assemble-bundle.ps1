#Requires -Version 5.1
<#
.SYNOPSIS
  Assemble the ETS2Nav Desktop bundle (P4R Batch 6A sections 6, 9, 10; Batch 6B sections 5, 8, 19).

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
      web/vendor/fonts/            optional glyphs  (see -FontsDir)
      data/europe-v5/              navigation dataset

  THREE properties this script deliberately enforces rather than assumes:

  1. The bundle is a PUBLIC artefact, so it must not carry the build machine's
     filesystem layout. Earlier revisions wrote `basemap.source = <path>` and
     `fonts.source = <path>`, which put `E:\Projects\...` / `C:\Users\...` into
     every bundle that shipped those resources. Provenance is now supplied as
     structured values (`-BasemapProvenance` / `-FontsProvenance`), and any
     absolute path inside it is rejected outright.

  2. The bundle profile is a measured fact, not a label. `profile` is FULL only
     when the dataset, the basemap and the glyphs are all actually present;
     otherwise it is CORE. A CORE bundle must never be described as a complete
     offline product.

  3. The shipped binaries must not contain fault-injection hooks. Both hooks
     (`ETS2NAV_FAULT_INJECT` in the sidecar, `ETS2NAV_JOB_FAULT` in the Desktop)
     are scanned for as literal byte sequences and rejected. The RC's release
     property "no injection surface in the shipped artefact" is therefore
     established by inspection of the exact bytes being shipped.

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
  Optional basemap archive to place at web/map.pmtiles. Requires -BasemapProvenance.

.PARAMETER BasemapProvenance
  JSON file describing where the basemap came from. Required keys: kind, dataset,
  source_commit, game_version.

.PARAMETER FontsDir
  Optional glyph directory to place at web/vendor/fonts. Requires -FontsProvenance.

.PARAMETER FontsProvenance
  JSON file describing the font. Required keys: font, version, upstream,
  license_spdx, source_sha256, generator, generator_version, generation_command,
  ranges.

.PARAMETER SourceDateEpoch
  Unix seconds. When supplied, `generated_at` is derived from it and every copied
  file's modification time is normalised to that instant, which is what makes two
  independent packagings produce identical payload bytes and identical archive
  entry timestamps (Batch 6B section 17).

.EXITCODES
  0 bundle assembled; 1 FAIL; 3 precondition (missing input, version drift,
  provenance invalid, injection hook present).
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$OutDir,
    [Parameter(Mandatory)][string]$Dataset,
    [string]$WebRoot,
    [ValidateSet('release', 'debug')][string]$Profile = 'release',
    [switch]$SkipBuild,
    [string]$MapPmtiles,
    [string]$BasemapProvenance,
    [string]$FontsDir,
    [string]$FontsProvenance,
    [int64]$SourceDateEpoch = 0
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

function Stop-Precondition {
    param([Parameter(Mandatory)][string]$Message)
    Write-Host "PRECONDITION FAILURE: $Message"
    exit 3
}

# ── version consistency (section 19) ─────────────────────────────────────────
# desktop/Cargo.toml is the single source of truth: `CARGO_PKG_VERSION` is what the
# Desktop prints and what the binary reports, so deriving everything from it is the
# only arrangement in which the display version cannot drift. The other three files
# must agree; a mismatch is a packaging defect, not something to reconcile silently.
function Get-CargoPackageVersion {
    param([Parameter(Mandatory)][string]$Path)
    $inPackage = $false
    foreach ($line in (Get-Content -LiteralPath $Path)) {
        if ($line -match '^\s*\[package\]\s*$') { $inPackage = $true; continue }
        if ($line -match '^\s*\[') { $inPackage = $false; continue }
        if ($inPackage -and $line -match '^\s*version\s*=\s*"([^"]+)"') { return $Matches[1] }
    }
    Stop-Precondition "无法从 $Path 读取 [package] version"
}

$versionSources = [ordered]@{}
$versionSources['desktop/Cargo.toml'] = Get-CargoPackageVersion -Path (Join-Path $desktopDir 'Cargo.toml')
$versionSources['nav-core/tools/nav-core-cli/Cargo.toml'] =
    Get-CargoPackageVersion -Path (Join-Path $navCore 'tools/nav-core-cli/Cargo.toml')
$versionSources['desktop/tauri.conf.json'] =
    (Get-Content -LiteralPath (Join-Path $desktopDir 'tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version
$versionSources['tools/ets2nav-web/package.json'] =
    (Get-Content -LiteralPath (Join-Path $repoRoot 'tools/ets2nav-web/package.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version

$appVersion = $versionSources['desktop/Cargo.toml']
$drift = @()
foreach ($k in $versionSources.Keys) {
    if ($versionSources[$k] -ne $appVersion) { $drift += "$k=$($versionSources[$k])" }
}
if ($drift.Count -gt 0) {
    Stop-Precondition "版本漂移：desktop/Cargo.toml=$appVersion，但 $($drift -join '，')"
}
Write-Host "== version $appVersion（4 处元数据一致）"

$cargoArgs = @()
if ($Profile -eq 'release') { $cargoArgs += '--release' }
$profileDir = if ($Profile -eq 'release') { 'release' } else { 'debug' }

if (-not $SkipBuild) {
    Invoke-Step 'build nav-core-cli (sidecar)' {
        Push-Location $navCore
        try { & cargo build @cargoArgs -p nav-core-cli } finally { Pop-Location }
    }
    Invoke-Step 'build ets2nav-desktop' {
        & cargo build @cargoArgs --manifest-path (Join-Path $desktopDir 'Cargo.toml')
    }
}

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
        Stop-Precondition "dataset is missing $req ($Dataset)"
    }
}

# ── fault-injection hooks must not ship (sections 4, 29) ─────────────────────
# Byte-level scan. Read as Latin-1 so every byte maps to one character and the
# ASCII literals cannot be split by a decoding substitution.
function Assert-NoAsciiLiteral {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$Literal,
        [Parameter(Mandatory)][string]$Why
    )
    $bytes = [IO.File]::ReadAllBytes($Path)
    $text = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
    if ($text.Contains($Literal)) {
        Stop-Precondition "$(Split-Path -Leaf $Path) 含故障注入字面量 '$Literal'（$Why）；发布产物不得包含注入面"
    }
}
Assert-NoAsciiLiteral -Path $desktopExeSrc -Literal 'ETS2NAV_JOB_FAULT' -Why 'Desktop 作业对象注入开关'
Assert-NoAsciiLiteral -Path $sidecarSrc -Literal 'ETS2NAV_FAULT_INJECT' -Why 'sidecar 数据源注入开关'
Assert-NoAsciiLiteral -Path $sidecarSrc -Literal 'ETS2NAV_FAULT_HOLD_MS' -Why 'sidecar 停机延迟注入开关'
Write-Host '== fault-injection hooks absent from both binaries'

# ── optional resource provenance (section 5) ─────────────────────────────────
# A drive-absolute path is "<single letter>:" + separator, not preceded by a scheme
# character. The lookbehind class is what keeps `https://` from matching: in
# "https://" the character before the `s` is `p`, which is in the class.
$absolutePathPattern = '(^|[^A-Za-z0-9+.\-])[A-Za-z]:[\\/]'

function Read-Provenance {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string[]]$RequiredKeys,
        [Parameter(Mandatory)][string]$Context
    )
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        Stop-Precondition "$Context provenance 文件不存在: $Path"
    }
    $obj = Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json
    $names = @($obj.PSObject.Properties.Name)
    foreach ($k in $RequiredKeys) {
        if ($names -notcontains $k) {
            Stop-Precondition "$Context provenance 缺键 '$k'（实际键：$($names -join ', ')）"
        }
        $v = $obj.$k
        if ($null -eq $v -or -not ($v -is [string]) -or $v.Trim().Length -eq 0) {
            Stop-Precondition "$Context provenance 的 '$k' 必须是非空字符串（实际: $v）"
        }
        $v = $v.Trim()
        if ($v -match $absolutePathPattern -or $v -match '^\\\\' -or $v -match '^/(home|Users)/') {
            Stop-Precondition "$Context provenance 的 '$k' 是绝对路径（$v）；provenance 不得记录调用机器的文件系统位置"
        }
    }
    return $obj
}

# Extra keys beyond the required set are preserved, so a caller may carry more
# provenance (tool versions, build ids) without this script having to know them.
function Select-ProvenanceObject {
    param([Parameter(Mandatory)]$Source, [Parameter(Mandatory)][string]$Context)
    $o = [ordered]@{}
    foreach ($p in $Source.PSObject.Properties) {
        $v = $p.Value
        if ($null -eq $v) { continue }
        if ($v -is [string]) {
            if ($v -match $absolutePathPattern -or $v -match '^\\\\' -or $v -match '^/(home|Users)/') {
                Stop-Precondition "$Context provenance 的 '$($p.Name)' 是绝对路径（$v）"
            }
        }
        $o[$p.Name] = $v
    }
    return $o
}

if ($MapPmtiles -and -not $BasemapProvenance) {
    Stop-Precondition '提供了 -MapPmtiles 但没有 -BasemapProvenance：发布资源必须有可核查的来源，不得只记录本机路径'
}
if ($FontsDir -and -not $FontsProvenance) {
    Stop-Precondition '提供了 -FontsDir 但没有 -FontsProvenance：发布资源必须有可核查的来源，不得只记录本机路径'
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
$basemapProvRecord = $null
if ($MapPmtiles) {
    if (-not (Test-Path -LiteralPath $MapPmtiles -PathType Leaf)) {
        Stop-Precondition "-MapPmtiles not found: $MapPmtiles"
    }
    $prov = Read-Provenance -Path $BasemapProvenance -Context 'basemap' `
        -RequiredKeys @('kind', 'dataset', 'source_commit', 'game_version')
    Copy-Item -LiteralPath $MapPmtiles -Destination (Join-Path $webOut 'map.pmtiles') -Force
    $id = Get-FileIdentity -Path (Join-Path $webOut 'map.pmtiles')
    $basemapPresent = $true
    $basemapSha = $id.Sha256
    $basemapBytes = $id.Bytes
    $basemapProvRecord = Select-ProvenanceObject -Source $prov -Context 'basemap'
    Write-Host "   basemap: $($id.Bytes) B sha256=$($id.Sha256) kind=$($basemapProvRecord['kind'])"
} else {
    Write-Host '   basemap: NOT SUPPLIED (recorded as absent in the manifest)'
}

$fontsPresent = $false
$fontsFiles = 0
$fontsBytes = 0
$fontsDigest = $null
$fontsProvRecord = $null
if ($FontsDir) {
    if (-not (Test-Path -LiteralPath $FontsDir)) {
        Stop-Precondition "-FontsDir not found: $FontsDir"
    }
    $prov = Read-Provenance -Path $FontsProvenance -Context 'fonts' -RequiredKeys @(
        'font', 'version', 'upstream', 'license_spdx', 'source_sha256',
        'generator', 'generator_version', 'generation_command', 'ranges'
    )
    if ($prov.source_sha256 -notmatch '^[0-9a-fA-F]{64}$') {
        Stop-Precondition "fonts provenance 的 source_sha256 必须是 64 位十六进制（实际: $($prov.source_sha256)）"
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
    $fontsDigest = Get-TreeDigest -Entries $fe

    # ── 声明与磁盘必须互相覆盖（Batch 6B §7）────────────────────────────────
    # 只要求 `ranges` 是非空字符串，等于允许「声明 0-255 而实际放任意同名文件」通过
    # 全部检查——那正是「以存在性代替覆盖」在打包层的残留形态。此处双向核对：
    #   正向：provenance 声明的每个 range 都必须有 <font>/<range>.pbf；
    #   反向：磁盘上每个分片都必须被声明覆盖，且所在目录名必须等于声明的字体。
    # 反向不可省略：否则多带一套未声明的字形数据也能出厂，而清单只记录文件数。
    $declaredRanges = @(
        $prov.ranges -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ }
    )
    if ($declaredRanges.Count -eq 0) {
        Stop-Precondition "fonts provenance 的 ranges 未给出任何分片范围（实际: '$($prov.ranges)'）"
    }
    foreach ($r in $declaredRanges) {
        if ($r -notmatch '^\d+-\d+$') {
            Stop-Precondition "fonts provenance 的 ranges 元素 '$r' 不是 'lo-hi' 形式"
        }
    }
    $onDisk = @{}
    foreach ($f in (Get-ChildItem -LiteralPath $fontsOut -Recurse -File -Filter '*.pbf')) {
        $stem = [IO.Path]::GetFileNameWithoutExtension($f.Name)
        $stack = Split-Path -Leaf (Split-Path -Parent $f.FullName)
        $onDisk["$stack/$stem"] = $true
    }
    if ($onDisk.Count -eq 0) {
        Stop-Precondition "提供了 -FontsDir 但 web/vendor/fonts 下没有任何 .pbf 分片（$FontsDir）"
    }
    foreach ($r in $declaredRanges) {
        $key = "$($prov.font)/$r"
        if (-not $onDisk.ContainsKey($key)) {
            Stop-Precondition "字形分片缺失：provenance 声明 font='$($prov.font)' ranges='$($prov.ranges)'，但 $key.pbf 不存在；磁盘上实际有：$((@($onDisk.Keys) | Sort-Object) -join ', ')"
        }
    }
    foreach ($key in @($onDisk.Keys)) {
        $parts = $key -split '/', 2
        if ($parts[0] -ne $prov.font) {
            Stop-Precondition "字形分片目录与声明的字体不符：磁盘 'web/vendor/fonts/$key.pbf' vs provenance font='$($prov.font)'"
        }
        if ($declaredRanges -notcontains $parts[1]) {
            Stop-Precondition "字形分片未被 provenance 声明覆盖：'$key.pbf' 不在 ranges='$($prov.ranges)' 内"
        }
    }
    Write-Host "   glyphs : 声明与磁盘互相覆盖（font='$($prov.font)' ranges='$($prov.ranges)'，磁盘 $($onDisk.Count) 个分片）"

    $fontsProvRecord = Select-ProvenanceObject -Source $prov -Context 'fonts'
    Write-Host "   glyphs : $($ft.Files) files $($ft.Bytes) B tree=$($fontsDigest.Substring(0,16))... -> web/vendor/fonts"
} else {
    Write-Host '   glyphs : NOT SUPPLIED (recorded as absent in the manifest)'
}

# ── source date normalisation (section 17) ───────────────────────────────────
if ($SourceDateEpoch -gt 0) {
    $stamp = [DateTimeOffset]::FromUnixTimeSeconds($SourceDateEpoch).UtcDateTime
    $normalised = 0
    foreach ($f in (Get-ChildItem -LiteralPath $OutDir -Recurse -File -Force)) {
        $f.LastWriteTimeUtc = $stamp
        $normalised += 1
    }
    Write-Host "== normalised $normalised file timestamps to $($stamp.ToString('yyyy-MM-ddTHH:mm:ssZ'))"
    $generatedAt = $stamp.ToString('yyyy-MM-ddTHH:mm:ssZ')
} else {
    $generatedAt = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
}

# ── profile (section 8) ──────────────────────────────────────────────────────
# Measured, not declared. FULL requires all three resource classes to be present.
$bundleProfile = if ($basemapPresent -and $fontsPresent) { 'FULL' } else { 'CORE' }
Write-Host "== bundle profile=$bundleProfile (basemap=$basemapPresent fonts=$fontsPresent)"

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
$dsManifest = Get-Content -LiteralPath (Join-Path $datasetOut 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$source = Get-SourceCommit -RepoRoot $repoRoot

# NOTE: no key in this manifest may carry a filesystem path. `bundle-manifest.json`
# ships inside the public artefact, so a path here is a privacy leak, and the
# release privacy scanner treats it as a failure.
$manifest = [ordered]@{
    schema       = 2
    product      = 'ETS2Nav'
    app_version  = $appVersion
    profile      = $bundleProfile
    generated_at = $generatedAt
    source       = [ordered]@{
        commit        = $source.Commit
        dirty         = $source.Dirty
        cargo_profile = $Profile
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
        path       = 'web/map.pmtiles'
        present    = $basemapPresent
        sha256     = $basemapSha
        bytes      = $basemapBytes
        provenance = $basemapProvRecord
    }
    fonts        = [ordered]@{
        dir         = 'web/vendor/fonts'
        present     = $fontsPresent
        files       = $fontsFiles
        bytes       = $fontsBytes
        tree_sha256 = $fontsDigest
        provenance  = $fontsProvRecord
    }
}

Write-JsonNoBom -Path (Join-Path $OutDir 'bundle-manifest.json') -Object $manifest

Write-Host ''
Write-Host '=== bundle assembled ==='
Write-Host "dir       : $OutDir"
Write-Host "version   : $appVersion  profile=$bundleProfile"
Write-Host "source    : commit=$($source.Commit) dirty=$($source.Dirty) cargo_profile=$Profile"
Write-Host "desktop   : $($desktopId.Bytes) B sha256=$($desktopId.Sha256)"
Write-Host "sidecar   : $($sidecarId.Bytes) B sha256=$($sidecarId.Sha256)"
Write-Host "web       : $($webTotals.Files) files $($webTotals.Bytes) B tree=$($webDigest.Substring(0,16))..."
Write-Host "dataset   : $($datasetTotals.Files) files $($datasetTotals.Bytes) B tree=$($datasetDigest.Substring(0,16))..."
Write-Host "basemap   : present=$basemapPresent"
Write-Host "fonts     : present=$fontsPresent"
exit 0
