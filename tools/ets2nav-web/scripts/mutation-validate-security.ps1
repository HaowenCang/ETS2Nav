# P4R Batch 3 section 22: security mutation validation.
#
# Proves the security tests can actually DETECT a security regression -- a test that
# passes on both the correct and the broken code proves nothing.
#
# For each mutation: apply a tiny break -> run the matching suite -> require exit != 0;
# restore -> run again -> require exit == 0. Nothing broken is ever left behind: every
# touched file is restored from a temp backup in a finally block and verified by SHA-256.
#
# Two families of mutations:
#   Rust (server)   -> verified by `npm run test:security` (verify-lan-security.py)
#   app.js (client) -> verified by the Playwright LAN spec
# A Rust mutation therefore needs `cargo build` before the suite can observe it.
#
# ASCII-ONLY, AND ALL ANCHORS MUST BE ASCII TOO. Windows PowerShell 5.1 reads .ps1 as
# ANSI unless it has a BOM, so any non-ASCII byte in this file (including inside a match
# anchor that quotes a source comment) is decoded as garbage and breaks the parser.
# ErrorActionPreference stays Continue: cargo/npm/npx write progress to stderr, and with
# 'Stop' PowerShell 5.1 turns those native stderr lines into terminating errors. Every
# step is checked through $LASTEXITCODE instead.
$ErrorActionPreference = "Continue"

# Repo paths are derived from this script's own location, not hardcoded: hardcoding an
# absolute developer path would make the script fail on any other checkout and would put
# a local private path into a committed test tool.
# $PSScriptRoot = <repo>\tools\ets2nav-web\scripts
$web = Split-Path $PSScriptRoot -Parent
$repo = Split-Path (Split-Path $web -Parent) -Parent
$navcore = Join-Path $repo "nav-core"
$secSrc = Join-Path $navcore "tools\nav-core-cli\src\security.rs"
$cliSrc = Join-Path $navcore "tools\nav-core-cli\src\server_cli.rs"
$appSrc = Join-Path $web "app.js"
$bakDir = Join-Path $env:TEMP "ets2nav-secmut"
$log = Join-Path $env:TEMP "ets2nav-security-mutation.log"
New-Item -ItemType Directory -Force -Path $bakDir | Out-Null
$results = @()
"security mutation validation started $(Get-Date -Format o)" | Set-Content $log -Encoding UTF8

function Backup([string]$path) {
  $name = [System.IO.Path]::GetFileName($path)
  Copy-Item $path (Join-Path $bakDir $name) -Force
}
function Restore([string]$path) {
  $name = [System.IO.Path]::GetFileName($path)
  Copy-Item (Join-Path $bakDir $name) $path -Force
  # Copy-Item also restores the ORIGINAL LastWriteTime, which is older than the build
  # artifact produced from the mutated source. Cargo then considers the crate fresh and
  # silently keeps the MUTATED binary: the "restored" run keeps failing and the whole
  # mutation check reports FAIL even though the source was restored correctly. This was
  # observed for real -- after a run the source hashes matched the backups while the
  # binary still served "Access-Control-Allow-Origin: *". Touch the file so the rebuild
  # is guaranteed, and Assert-FreshArtifact below verifies it actually happened.
  (Get-Item $path).LastWriteTime = Get-Date
}
function Hash([string]$path) { (Get-FileHash $path -Algorithm SHA256).Hash }

function Set-Mutation([string]$path, [string]$old, [string]$new, [string]$name) {
  $text = Get-Content $path -Raw -Encoding UTF8
  $count = ([regex]::Matches($text, [regex]::Escape($old))).Count
  if ($count -ne 1) { throw "mutation '$name': match count = $count in $path (expected 1)" }
  $text = $text.Replace($old, $new)
  [System.IO.File]::WriteAllText($path, $text, (New-Object System.Text.UTF8Encoding($false)))
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

# The binary must be NEWER than every source it is built from. Without this, a stale
# artifact makes the suite test the previous mutation while the source looks correct --
# a silent divergence between source and artifact, which is exactly what made the first
# run of this script report false FAILs for M1/M2/M3.
function Assert-FreshArtifact {
  $bin = Join-Path $navcore "target\debug\nav-core-cli.exe"
  if (-not (Test-Path $bin)) { throw "artifact missing: $bin" }
  $binTime = (Get-Item $bin).LastWriteTime
  foreach ($p in @($secSrc, $cliSrc)) {
    $srcTime = (Get-Item $p).LastWriteTime
    if ($binTime -lt $srcTime) {
      throw "stale artifact: $bin ($binTime) is older than $p ($srcTime)"
    }
  }
}

function BuildDist {
  Push-Location $web
  try {
    $out = & npm run build 2>&1 | Out-String
    $code = $LASTEXITCODE
    Add-Content $log "--- npm run build exit=$code`n$out"
    if ($code -ne 0) { throw "npm run build failed" }
  } finally { Pop-Location }
}

function Invoke-Security {
  Push-Location $web
  try {
    $out = & npm run test:security 2>&1 | Out-String
    $code = $LASTEXITCODE
    $tail = ($out -split "`n" | Where-Object { $_ -match 'FAIL=|SECURITY:|FAIL:' } | Select-Object -Last 3) -join " | "
    Add-Content $log "--- test:security exit=$code`n$out"
    return @{ code = $code; tail = $tail }
  } finally { Pop-Location }
}

function Invoke-LanE2E {
  Push-Location $web
  try {
    $out = & npx playwright test "12-lan" --reporter=line 2>&1 | Out-String
    $code = $LASTEXITCODE
    $tail = ($out -split "`n" | Where-Object { $_ -match '\S' } | Select-Object -Last 2) -join " | "
    Add-Content $log "--- playwright 12-lan exit=$code`n$out"
    return @{ code = $code; tail = $tail }
  } finally { Pop-Location }
}

Backup $secSrc; Backup $cliSrc; Backup $appSrc

# M1: let the route endpoint skip the authorization decision entirely.
# (ServerCtx::decide lives in server_cli.rs, not security.rs.)
$m1old = '        if !security::is_protected_api(path) {'
$m1new = '        if path == "/api/route" { return AuthOutcome::Allowed; } // MUTATION M1' + "`n" + '        if !security::is_protected_api(path) {'

# M2: force the WebSocket upgrade authorization result to Allowed.
$m2old = '        let outcome =' + "`n" + '            security::authorize(ctx.security.mode, peer, ctx.token(), presented.as_deref());'
$m2new = '        let outcome = { let _ = security::authorize(ctx.security.mode, peer, ctx.token(), presented.as_deref()); AuthOutcome::Allowed }; // MUTATION M2'

# M3: put the wildcard CORS header back for every origin.
$m3old = '    match origin.and_then(cors_allowed_origin) {' + "`n" + '        Some(matched) => vec!['
$m3new = '    if true { return vec![("Access-Control-Allow-Origin", "*".to_string())]; } // MUTATION M3' + "`n" + '    match origin.and_then(cors_allowed_origin) {' + "`n" + '        Some(matched) => vec!['

# M4: make the client derive the same-origin WS URL for loopback hosts only again.
$m4old = '  if (DESKTOP_HOSTS.includes(hostname)) return null;'
$m4new = '  if (DESKTOP_HOSTS.includes(hostname)) return null;' + "`n" + '  if (hostname !== "127.0.0.1" && hostname !== "localhost") return null; // MUTATION M4'

$mutations = @(
  @{ name = "M1 /api/route authorization bypassed";            file = $cliSrc; old = $m1old; new = $m1new; kind = "rust"; suite = "security" },
  @{ name = "M2 remote WS token not verified";                  file = $cliSrc; old = $m2old; new = $m2new; kind = "rust"; suite = "security" },
  @{ name = "M3 CORS wildcard restored";                        file = $secSrc; old = $m3old; new = $m3new; kind = "rust"; suite = "security" },
  @{ name = "M4 remote same-origin WS reverted to loopback";    file = $appSrc; old = $m4old; new = $m4new; kind = "app";  suite = "lane2e"  }
)

try {
  foreach ($m in $mutations) {
    Write-Host ""
    Write-Host "===== $($m.name) ====="
    Set-Mutation $m.file $m.old $m.new $m.name
    if ($m.kind -eq "rust") { BuildRust } else { BuildDist }

    if ($m.suite -eq "security") { $bad = Invoke-Security } else { $bad = Invoke-LanE2E }
    Write-Host "  mutated  : exit=$($bad.code)  $($bad.tail)"

    Restore $m.file
    if ($m.kind -eq "rust") { BuildRust } else { BuildDist }
    if ($m.suite -eq "security") { $good = Invoke-Security } else { $good = Invoke-LanE2E }
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
  Restore $secSrc; Restore $cliSrc; Restore $appSrc
  BuildRust
  BuildDist
  $bad = 0
  foreach ($p in @($secSrc, $cliSrc, $appSrc)) {
    $name = [System.IO.Path]::GetFileName($p)
    $h1 = Hash $p
    $h2 = Hash (Join-Path $bakDir $name)
    if ($h1 -ne $h2) {
      Write-Host "[restore] FATAL: $name differs from backup after restore" -ForegroundColor Red
      $bad = 1
    } else {
      Write-Host "[restore] $name restored (sha256 $($h1.Substring(0,16)))"
    }
  }
  if ($bad -eq 1) { exit 2 }
}

Write-Host ""
Write-Host "===== security mutation validation summary ====="
$results | Format-Table -AutoSize
$failed = $results | Where-Object { $_.Verdict -ne "PASS" }
if ($failed) { Write-Host "SECURITY MUTATION VALIDATION FAIL"; exit 1 }
Write-Host "SECURITY MUTATION VALIDATION PASS"
exit 0
