#Requires -Version 5.1
<#
.SYNOPSIS
  Materialise the SCS Telemetry SDK 1.14 header tree from a fixed official source,
  verified against a pinned SHA-256 (P4R Batch 6a, release packaging).

.DESCRIPTION
  The two tracked telemetry plugin DLLs are built by telemetry-plugin/*/build.bat,
  which expects the SCS SDK headers under <repo>\vendor\scs_sdk_1_14\include. vendor/
  is gitignored, so a clean CI checkout has no headers and cannot rebuild the DLLs.
  That was recorded as a release-packaging blocker; this script is the reproducible
  way out of it.

  It is deliberately shaped like scripts/ci/prepare-dataset.ps1: the trust root is
  the pinned digest, never the cache and never the file name.

    1. Fixed URL and archive name as constants, pinned expected SHA-256.
    2. Download only if the cached archive is absent or fails the digest.
    3. Re-hash the cache on *every* run. A cache hit is not a verification; the
       digest is re-checked even when the archive was already there. Mismatch is
       fatal, before anything is extracted.
    4. Extract into a gitignored location (asserted to be gitignored, not assumed).
    5. Assert every header the plugin sources #include exists, plus the transitive
       closure of those headers inside the SDK, and that every extracted file is
       byte-identical to its archive entry.
    6. Print the resolved header directory and the exact value to hand to a caller.

  Exit codes: 0 = PASS; 2 = USAGE; 3 = EXTERNAL ASSET FAILURE (download failed,
  digest mismatch, structure incomplete). Any non-zero exit means the headers on
  disk must not be trusted for a release build.

.PARAMETER CacheDir
  Directory holding the cached archive. Defaults to <repo>\vendor (gitignored).

.PARAMETER ExtractDir
  Directory the archive is extracted into. Defaults to <repo>\vendor\scs_sdk_1_14,
  which is where telemetry-plugin/*/build.bat looks for the include tree.

.PARAMETER Offline
  Forbid downloading. The cached archive must already be present and must still pass
  the digest check; otherwise the script fails with exit 3. This never skips or
  weakens verification, it only removes the network fallback.

.EXAMPLE
  .\scripts\prepare-scs-sdk.ps1

.EXAMPLE
  .\scripts\prepare-scs-sdk.ps1 -Offline
#>
[CmdletBinding()]
param(
    [string]$CacheDir,
    [string]$ExtractDir,
    [switch]$Offline
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$EXIT_PASS = 0
$EXIT_USAGE = 2
$EXIT_ASSET = 3

# -- Provenance constants -----------------------------------------------------
# Official URL as listed by the SCS Modding Wiki for stable Telemetry SDK 1.14.
$SDK_VERSION = '1.14'
$ARCHIVE_NAME = 'scs_sdk_1_14.zip'
$OFFICIAL_URL = 'https://download.eurotrucksimulator2.com/scs_sdk_1_14.zip'

# Expected SHA-256 of the archive. Obtained by downloading $OFFICIAL_URL on
# 2026-09-13 and hashing the result; the archive already cached at
# vendor\scs_sdk_1_14.zip hashed to the same value, so the two agree. See
# telemetry-plugin/scs-sdk-provenance.json for the full record.
$EXPECTED_SHA256 = 'c6c1f7376b7324994d9f9c567f3c4141fbbf305b6bf803bc4cfeef2437b2023a'
$EXPECTED_SIZE = 62794

# Minimum header set: the quoted includes of the plugin sources are discovered at
# run time (so this check cannot drift away from the sources), and these five are
# additionally pinned here so that a source edit cannot silently shrink the check.
$MINIMUM_HEADERS = @(
    'scssdk_telemetry.h'
    'eurotrucks2/scssdk_eut2.h'
    'eurotrucks2/scssdk_telemetry_eut2.h'
    'amtrucks/scssdk_ats.h'
    'amtrucks/scssdk_telemetry_ats.h'
)

$PLUGIN_SOURCES = @(
    'telemetry-plugin/scs-nav-bridge/scs-nav-bridge.cpp'
    'telemetry-plugin/semaphore-bridge/semaphore-bridge.cpp'
)

function Write-Log { param([string]$Text = '') Write-Host $Text }

function Fail-Asset {
    param([string]$Message)
    Write-Log ''
    Write-Log "EXTERNAL ASSET FAILURE: $Message"
    exit $EXIT_ASSET
}

function Get-Sha256Lower {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-BytesSha256Lower {
    param([Parameter(Mandatory)][byte[]]$Bytes)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash($Bytes))).Replace('-', '').ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}

# Quoted #include directives of a C/C++ source file, in source order, de-duplicated.
function Get-QuotedIncludes {
    param([Parameter(Mandatory)][string]$Path)
    $seen = New-Object System.Collections.Generic.List[string]
    foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
        $m = [regex]::Match($line, '^\s*#\s*include\s+"([^"]+)"')
        if ($m.Success) {
            $name = $m.Groups[1].Value
            if (-not $seen.Contains($name)) { [void]$seen.Add($name) }
        }
    }
    return $seen
}

# -- Paths --------------------------------------------------------------------
if ($EXPECTED_SHA256 -notmatch '^[0-9a-f]{64}$') {
    Write-Log 'USAGE ERROR: the pinned EXPECTED_SHA256 constant is not 64 lowercase hex digits'
    exit $EXIT_USAGE
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $CacheDir) { $CacheDir = Join-Path $repoRoot 'vendor' }
if (-not $ExtractDir) { $ExtractDir = Join-Path $repoRoot 'vendor\scs_sdk_1_14' }
$CacheDir = [System.IO.Path]::GetFullPath($CacheDir)
$ExtractDir = [System.IO.Path]::GetFullPath($ExtractDir)
$archive = Join-Path $CacheDir $ARCHIVE_NAME
$includeDir = Join-Path $ExtractDir 'include'

Write-Log '========================================================================'
Write-Log 'SCS Telemetry SDK preparation (P4R Batch 6a, release packaging)'
Write-Log '========================================================================'
Write-Log "sdk version : $SDK_VERSION"
Write-Log "url         : $OFFICIAL_URL"
Write-Log "archive     : $ARCHIVE_NAME"
Write-Log "expected    : sha256:$EXPECTED_SHA256  ($EXPECTED_SIZE bytes)"
Write-Log "repo root   : $repoRoot"
Write-Log "cache dir   : $CacheDir"
Write-Log "extract dir : $ExtractDir"
Write-Log "offline     : $($Offline.IsPresent)"
Write-Log ''

# -- 0. The cache and extract dirs must be gitignored -------------------------
# "Extract into a gitignored location" is asserted rather than assumed: a future
# .gitignore edit that stops ignoring vendor/ would otherwise start staging a
# third-party SDK into the repository without anyone noticing.
if (Test-Path -LiteralPath (Join-Path $repoRoot '.git')) {
    Push-Location $repoRoot
    try {
        foreach ($probe in @('vendor/.b6a-gitignore-probe', 'vendor/scs_sdk_1_14/.b6a-gitignore-probe')) {
            & git check-ignore -q -- $probe
            if ($LASTEXITCODE -ne 0) {
                Fail-Asset "vendor/ is no longer gitignored (git check-ignore rejected '$probe'). " +
                           'The SDK must never be staged for commit; fix .gitignore before proceeding.'
            }
        }
        Write-Log 'OK: cache/extract locations are gitignored (verified with git check-ignore)'
    } finally {
        Pop-Location
    }
} else {
    Write-Log 'NOTE: no .git directory here, skipped the gitignore assertion'
}

# -- 1/2. Cache: always re-hash, download only when necessary -----------------
New-Item -ItemType Directory -Path $CacheDir -Force | Out-Null

$needDownload = $true
if (Test-Path -LiteralPath $archive -PathType Leaf) {
    Write-Log ''
    Write-Log 'Cached archive found; re-verifying its digest (the cache is not a trust root) ...'
    $cached = Get-Sha256Lower -Path $archive
    $cachedSize = (Get-Item -LiteralPath $archive).Length
    Write-Log "  cached     : sha256:$cached  ($cachedSize bytes)"
    if ($cached -eq $EXPECTED_SHA256) {
        Write-Log '  cache digest matches the pinned value; download not needed'
        $needDownload = $false
    } else {
        Write-Log '  cache digest does NOT match; discarding the cached file'
        Remove-Item -LiteralPath $archive -Force
    }
}

if ($needDownload) {
    if ($Offline) {
        Fail-Asset ("archive is not present/valid in the cache and -Offline was given.`n" +
            "  expected path : $archive`n" +
            "  expected      : sha256:$EXPECTED_SHA256`n" +
            "  Run without -Offline to download from $OFFICIAL_URL")
    }
    Write-Log ''
    Write-Log "Downloading $OFFICIAL_URL ..."
    # Windows PowerShell 5.1 does not negotiate TLS 1.2 by default.
    try {
        [Net.ServicePointManager]::SecurityProtocol =
            [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch {
        Write-Log "NOTE: could not adjust SecurityProtocol ($($_.Exception.Message))"
    }
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $client = New-Object System.Net.WebClient
        $client.Headers.Add('User-Agent', 'ets2nav-prepare-scs-sdk')
        $client.DownloadFile($OFFICIAL_URL, $archive)
        $client.Dispose()
    } catch {
        Fail-Asset "download failed: $OFFICIAL_URL`n  $($_.Exception.Message)"
    }
    $sw.Stop()
    Write-Log ("  downloaded {0} bytes in {1:N1}s" -f (Get-Item -LiteralPath $archive).Length, $sw.Elapsed.TotalSeconds)
}

# -- 3. Digest check: mismatch is fatal, nothing is extracted -----------------
Write-Log ''
$actual = Get-Sha256Lower -Path $archive
$actualSize = (Get-Item -LiteralPath $archive).Length
Write-Log "actual      : sha256:$actual  ($actualSize bytes)"
if ($actual -ne $EXPECTED_SHA256) {
    Fail-Asset ("archive digest mismatch.`n" +
        "  url      : $OFFICIAL_URL`n" +
        "  archive  : $archive`n" +
        "  expected : sha256:$EXPECTED_SHA256`n" +
        "  actual   : sha256:$actual`n" +
        "  Nothing was extracted. Either the upstream archive changed or this is not`n" +
        "  the official file; do not build a release artefact from it.")
}
if ($actualSize -ne $EXPECTED_SIZE) {
    Fail-Asset ("archive size mismatch: expected $EXPECTED_SIZE bytes, observed $actualSize bytes`n" +
        '  (the digest matched, so this indicates a wrong EXPECTED_SIZE constant)')
}
Write-Log 'digest and size verified'

# -- 4. Extract into a staging directory --------------------------------------
$staging = Join-Path $CacheDir ('scs_sdk_' + $SDK_VERSION + '.staging')
if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
New-Item -ItemType Directory -Path $staging -Force | Out-Null

Write-Log ''
Write-Log "Extracting to staging directory: $staging"
$zip = $null
try {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($archive)
    $files = @($zip.Entries | Where-Object { $_.Name -ne '' })
    Write-Log "  archive holds $($zip.Entries.Count) entries ($($files.Count) files)"
    $extractFailures = @()
    foreach ($entry in $files) {
        $dest = Join-Path $staging ($entry.FullName -replace '/', '\')
        $parent = Split-Path -Parent $dest
        if ($parent -and -not (Test-Path -LiteralPath $parent)) {
            New-Item -ItemType Directory -Path $parent -Force | Out-Null
        }
        try {
            [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $dest, $true)
        } catch {
            $extractFailures += "$($entry.FullName): $($_.Exception.Message)"
        }
    }
    if ($extractFailures.Count -gt 0) {
        $zip.Dispose(); $zip = $null
        Fail-Asset ("extraction failed for $($extractFailures.Count) entries:`n  " + ($extractFailures -join "`n  "))
    }

    # -- 5a. Every extracted file must be byte-identical to its archive entry --
    $byteMismatch = @()
    foreach ($entry in $files) {
        $dest = Join-Path $staging ($entry.FullName -replace '/', '\')
        if (-not (Test-Path -LiteralPath $dest -PathType Leaf)) {
            $byteMismatch += "missing after extraction: $($entry.FullName)"
            continue
        }
        $ms = New-Object System.IO.MemoryStream
        try {
            $s = $entry.Open()
            try { $s.CopyTo($ms) } finally { $s.Dispose() }
            $entryHash = Get-BytesSha256Lower -Bytes $ms.ToArray()
        } finally {
            $ms.Dispose()
        }
        $diskHash = Get-Sha256Lower -Path $dest
        if ($entryHash -ne $diskHash) { $byteMismatch += "content differs: $($entry.FullName)" }
    }
    if ($byteMismatch.Count -gt 0) {
        $zip.Dispose(); $zip = $null
        Fail-Asset ("extracted tree is not byte-identical to the archive:`n  " + ($byteMismatch -join "`n  "))
    }
    Write-Log "  all $($files.Count) extracted files are byte-identical to their archive entry"
} catch {
    if ($zip) { $zip.Dispose(); $zip = $null }
    Fail-Asset "extraction stage failed: $($_.Exception.Message)"
} finally {
    if ($zip) { $zip.Dispose() }
}

# -- 5b. Required headers -----------------------------------------------------
# Discovered from the plugin sources so the check follows the sources, unioned
# with the pinned minimum so a source edit cannot shrink it, then closed
# transitively over the SDK's own #include graph (the compiler needs all of them).
$required = New-Object System.Collections.Generic.List[string]
foreach ($h in $MINIMUM_HEADERS) { if (-not $required.Contains($h)) { [void]$required.Add($h) } }
foreach ($rel in $PLUGIN_SOURCES) {
    $srcPath = Join-Path $repoRoot ($rel -replace '/', '\')
    if (-not (Test-Path -LiteralPath $srcPath -PathType Leaf)) {
        Fail-Asset "plugin source not found: $srcPath"
    }
    foreach ($inc in (Get-QuotedIncludes -Path $srcPath)) { [void]$required.Add($inc) }
}
$direct = @($required | Sort-Object -Unique)

$queue = New-Object System.Collections.Generic.Queue[string]
foreach ($h in $direct) { $queue.Enqueue($h) }
$closure = New-Object System.Collections.Generic.List[string]
while ($queue.Count -gt 0) {
    $h = $queue.Dequeue()
    if ($closure.Contains($h)) { continue }
    [void]$closure.Add($h)
    $p = Join-Path (Join-Path $staging 'include') ($h -replace '/', '\')
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { continue }  # reported below
    foreach ($inc in (Get-QuotedIncludes -Path $p)) {
        $resolved = $inc -replace '\\', '/'
        $resolved = ($h -replace '[^/]+$', '') + $resolved   # relative to the including header
        $parts = New-Object System.Collections.Generic.List[string]
        foreach ($seg in ($resolved -split '/')) {
            if ($seg -eq '' -or $seg -eq '.') { continue }
            if ($seg -eq '..') { if ($parts.Count -gt 0) { $parts.RemoveAt($parts.Count - 1) }; continue }
            [void]$parts.Add($seg)
        }
        $norm = ($parts -join '/')
        if (-not $closure.Contains($norm)) { $queue.Enqueue($norm) }
    }
}
$allNeeded = @(($direct + $closure) | Sort-Object -Unique)

$missing = @()
foreach ($h in $allNeeded) {
    $p = Join-Path (Join-Path $staging 'include') ($h -replace '/', '\')
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { $missing += $h }
    elseif ((Get-Item -LiteralPath $p).Length -le 0) { $missing += "$h (zero bytes)" }
}
if ($missing.Count -gt 0) {
    Fail-Asset ("required headers are absent from the archive:`n  " + ($missing -join "`n  ") +
        "`n  The pinned digest is $EXPECTED_SHA256, so this archive is not the SDK this`n" +
        '  repository builds against.')
}
Write-Log ''
Write-Log "header check: $($direct.Count) direct includes resolved, $($allNeeded.Count) headers present in total"
foreach ($h in $direct) {
    $p = Join-Path (Join-Path $staging 'include') ($h -replace '/', '\')
    Write-Log ("  OK  {0,-42} {1,6} bytes" -f $h, (Get-Item -LiteralPath $p).Length)
}

# -- 6. Publish staging into place --------------------------------------------
if (Test-Path -LiteralPath $ExtractDir) { Remove-Item -LiteralPath $ExtractDir -Recurse -Force }
Move-Item -LiteralPath $staging -Destination $ExtractDir

$published = Join-Path $ExtractDir 'include'
$postMissing = @()
foreach ($h in $allNeeded) {
    $p = Join-Path $published ($h -replace '/', '\')
    if (-not (Test-Path -LiteralPath $p -PathType Leaf)) { $postMissing += $h }
}
if ($postMissing.Count -gt 0) {
    Fail-Asset ("headers missing after publish to ${ExtractDir}:`n  " + ($postMissing -join "`n  "))
}

Write-Log ''
Write-Log '========================================================================'
Write-Log "SDK ready (Telemetry SDK $SDK_VERSION, archive sha256:$actual)"
Write-Log "HEADER_DIR=$published"
Write-Log "  build.bat expects this at: <repo>\vendor\scs_sdk_1_14\include"
Write-Log "  callers can pass it on as: -SdkInclude `"$published`""
Write-Log 'exit 0 (0=PASS 2=USAGE 3=EXTERNAL ASSET FAILURE)'
Write-Log '========================================================================'
exit $EXIT_PASS
