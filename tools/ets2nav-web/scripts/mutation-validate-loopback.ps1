# P4R Batch 3.5 section 20: browser-to-loopback mutation validation.
#
# Proves the NEW Batch 3.5 tests can actually DETECT the vulnerability class they were
# written for. A suite that passes on both the hardened and the vulnerable server proves
# nothing -- and this round exists precisely because such a gap was found by an
# independent review of Batch 3.
#
# For each mutation: apply a tiny break -> run the matching suite -> require exit != 0;
# restore -> run again -> require exit == 0. Every touched file is restored from a temp
# backup in a finally block and verified by SHA-256, and the build artifact is checked to
# be newer than its sources (a stale artifact silently tests the previous mutation --
# observed for real during Batch 3).
#
# Detector mapping (deliberately matching the tests the task book names):
#   M1 Loopback -> Allowed without token   -> npm run test:browser-loopback (BLS-01/BLS-03)
#   M2 WebSocket Origin check disabled     -> npm run test:browser-loopback (BLS-04)
#   M3 text/plain accepted for /api/route  -> npm run test:security        (S3c, 415 policy)
#
# ASCII-ONLY, AND ALL ANCHORS MUST BE ASCII TOO. Windows PowerShell 5.1 reads .ps1 as
# ANSI unless it has a BOM, so any non-ASCII byte in this file (including inside a match
# anchor that quotes a source comment) is decoded as garbage and breaks the parser.
# ErrorActionPreference stays Continue: cargo/npm/npx write progress to stderr, and with
# 'Stop' PowerShell 5.1 turns those native stderr lines into terminating errors. Every
# step is checked through $LASTEXITCODE instead.
$ErrorActionPreference = "Continue"

# Paths are derived from this script's own location, never hardcoded: a hardcoded local
# path would break on any other checkout and would put a private developer path into a
# committed test tool.
# $PSScriptRoot = <repo>\tools\ets2nav-web\scripts
$web = Split-Path $PSScriptRoot -Parent
$repo = Split-Path (Split-Path $web -Parent) -Parent
$navcore = Join-Path $repo "nav-core"
$secSrc = Join-Path $navcore "tools\nav-core-cli\src\security.rs"
$cliSrc = Join-Path $navcore "tools\nav-core-cli\src\server_cli.rs"
$bakDir = Join-Path $env:TEMP "ets2nav-loopback-mut"
$log = Join-Path $env:TEMP "ets2nav-loopback-mutation.log"
New-Item -ItemType Directory -Force -Path $bakDir | Out-Null
$results = @()
"loopback mutation validation started $(Get-Date -Format o)" | Set-Content $log -Encoding UTF8

function Backup([string]$path) {
  Copy-Item $path (Join-Path $bakDir ([System.IO.Path]::GetFileName($path))) -Force
}
function Restore([string]$path) {
  Copy-Item (Join-Path $bakDir ([System.IO.Path]::GetFileName($path))) $path -Force
  # Copy-Item also restores the ORIGINAL LastWriteTime, which is older than the artifact
  # built from the mutated source; cargo then treats the crate as fresh and silently keeps
  # the MUTATED binary. Touch the file and verify with Assert-FreshArtifact.
  (Get-Item $path).LastWriteTime = Get-Date
}
function Hash([string]$path) { (Get-FileHash $path -Algorithm SHA256).Hash }

function Set-Mutation([string]$path, [string]$old, [string]$new, [string]$name) {
  $text = Get-Content $path -Raw -Encoding UTF8
  $count = ([regex]::Matches($text, [regex]::Escape($old))).Count
  if ($count -ne 1) { throw "mutation '$name': match count = $count in $path (expected 1)" }
  [System.IO.File]::WriteAllText($path, $text.Replace($old, $new), (New-Object System.Text.UTF8Encoding($false)))
}

function BuildRust {
  Push-Location $navcore
  try {
    $out = & cargo build --package nav-core-cli 2>&1 | Out-String
    $code = $LASTEXITCODE
    Add-Content $log "--- cargo build exit=$code`n$out"
    if ($code -ne 0) { throw "cargo build failed" }
  } finally { Pop-Location }
  Assert-FreshArtifact
}

function Assert-FreshArtifact {
  $bin = Join-Path $navcore "target\debug\nav-core-cli.exe"
  if (-not (Test-Path $bin)) { throw "artifact missing: $bin" }
  $binTime = (Get-Item $bin).LastWriteTime
  foreach ($p in @($secSrc, $cliSrc)) {
    if ($binTime -lt (Get-Item $p).LastWriteTime) {
      throw "stale artifact: $bin is older than $p"
    }
  }
}

function Invoke-BrowserSuite {
  Push-Location $web
  try {
    $out = & npm run test:browser-loopback -- --reporter=line 2>&1 | Out-String
    $code = $LASTEXITCODE
    $tail = ($out -split "`n" | Where-Object { $_ -match '\S' } | Select-Object -Last 2) -join " | "
    Add-Content $log "--- test:browser-loopback exit=$code`n$out"
    return @{ code = $code; tail = $tail }
  } finally { Pop-Location }
}

function Invoke-Security {
  Push-Location $web
  try {
    $out = & npm run test:security 2>&1 | Out-String
    $code = $LASTEXITCODE
    $tail = ($out -split "`n" | Where-Object { $_ -match 'FAIL=|SECURITY:|FAIL:' } | Select-Object -Last 2) -join " | "
    Add-Content $log "--- test:security exit=$code`n$out"
    return @{ code = $code; tail = $tail }
  } finally { Pop-Location }
}

Backup $secSrc; Backup $cliSrc

# M1: restore the Batch 3 model -- Loopback peers are Allowed without any token.
# This is the exact line whose absence is the Batch 3.5 fix; re-adding it re-opens both
# the cross-origin simple POST (BLS-01) and the cross-site WebSocket (BLS-03).
$m1old = '    if mode == ExposureMode::LoopbackOnly && peer == PeerClass::PrivateLan {'
$m1new = '    if peer == PeerClass::Loopback { return AuthOutcome::Allowed; } // MUTATION M1' + "`n" + $m1old

# M2: disable the WebSocket Origin check (token check stays intact).
# BLS-04 must fail: a valid token plus a hostile Origin must NOT be enough.
$m2old = '            if !ctx.security.origin_accepted(o) {'
$m2new = '            if !(true || ctx.security.origin_accepted(o)) { // MUTATION M2'

# M3: accept any media type for /api/route again (text/plain body parsed as JSON).
# Detected by verify-lan-security.py S3c, which requires 415 for non-JSON content types.
$m3old = '    if security::body_must_be_json(method, path) && !security::is_json_content_type(headers) {'
$m3new = '    if security::body_must_be_json(method, path) && !(true || security::is_json_content_type(headers)) { // MUTATION M3'

$mutations = @(
  @{ name = "M1 loopback allowed without token";      file = $secSrc; old = $m1old; new = $m1new; suite = "browser" },
  @{ name = "M2 WS Origin check disabled";            file = $cliSrc; old = $m2old; new = $m2new; suite = "browser" },
  @{ name = "M3 text/plain accepted for api/route";   file = $cliSrc; old = $m3old; new = $m3new; suite = "security" }
)

try {
  foreach ($m in $mutations) {
    Write-Host ""
    Write-Host "===== $($m.name) ====="
    Set-Mutation $m.file $m.old $m.new $m.name
    BuildRust

    if ($m.suite -eq "browser") { $bad = Invoke-BrowserSuite } else { $bad = Invoke-Security }
    Write-Host "  mutated  : exit=$($bad.code)  $($bad.tail)"

    Restore $m.file
    BuildRust
    if ($m.suite -eq "browser") { $good = Invoke-BrowserSuite } else { $good = Invoke-Security }
    Write-Host "  restored : exit=$($good.code)  $($good.tail)"

    $results += [PSCustomObject]@{
      Mutation     = $m.name
      Suite        = $m.suite
      MutatedExit  = $bad.code
      RestoredExit = $good.code
      Verdict      = if ($bad.code -ne 0 -and $good.code -eq 0) { "PASS" } else { "FAIL" }
    }
  }
} finally {
  Restore $secSrc; Restore $cliSrc
  BuildRust
  $bad = 0
  foreach ($p in @($secSrc, $cliSrc)) {
    $name = [System.IO.Path]::GetFileName($p)
    $h1 = Hash $p
    $h2 = Hash (Join-Path $bakDir $name)
    if ($h1 -ne $h2) {
      Write-Host "[restore] FATAL: $name differs from backup after restore"
      $bad = 1
    } else {
      Write-Host "[restore] $name restored (sha256 $($h1.Substring(0,16)))"
    }
  }
  if ($bad -eq 1) { exit 2 }
}

Write-Host ""
Write-Host "===== loopback mutation validation summary ====="
$results | Format-Table -AutoSize
$failed = $results | Where-Object { $_.Verdict -ne "PASS" }
if ($failed) { Write-Host "LOOPBACK MUTATION VALIDATION FAIL"; exit 1 }
Write-Host "LOOPBACK MUTATION VALIDATION PASS"
exit 0
