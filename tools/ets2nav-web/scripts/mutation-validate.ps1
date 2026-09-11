# P4R Batch 2 section 12: controlled mutation validation.
# For each mutation: apply a tiny break -> run the matching E2E -> require exit != 0;
# restore -> run again -> require exit == 0. No broken code is ever committed: the
# original app.js is restored from a temp backup in a finally block.
# ASCII-only on purpose: Windows PowerShell 5.1 reads .ps1 as ANSI unless it has a BOM.
#
# ErrorActionPreference stays Continue: npm/npx write progress to stderr, and with
# 'Stop' PowerShell 5.1 turns those native stderr lines into terminating errors.
# Every step is checked through $LASTEXITCODE instead.
$ErrorActionPreference = "Continue"
$web = "E:\Projects\Pi\ETS2Nav\tools\ets2nav-web"
$bak = Join-Path $env:TEMP "ets2nav-appjs-bak.js"
$src = Join-Path $web "app.js"
$log = Join-Path $env:TEMP "ets2nav-mutation.log"
$results = @()
"mutation validation started $(Get-Date -Format o)" | Set-Content $log -Encoding UTF8

function Invoke-E2E([string]$filter) {
  Push-Location $web
  try {
    $out = & npx playwright test $filter --reporter=line 2>&1 | Out-String
    $code = $LASTEXITCODE
    $tail = ($out -split "`n" | Where-Object { $_ -match '\S' } | Select-Object -Last 2) -join " | "
    Add-Content $log "--- spec=$filter exit=$code`n$out"
    return @{ code = $code; tail = $tail }
  } finally { Pop-Location }
}

function Build-Dist {
  Push-Location $web
  try {
    $out = & npm run build 2>&1 | Out-String
    $code = $LASTEXITCODE
    Add-Content $log "--- build exit=$code`n$out"
    if ($code -ne 0) { throw "npm run build failed" }
  } finally { Pop-Location }
}

function Set-Mutation([string]$old, [string]$new, [string]$name) {
  $text = Get-Content $src -Raw -Encoding UTF8
  $count = ([regex]::Matches($text, [regex]::Escape($old))).Count
  if ($count -ne 1) { throw "mutation '$name': match count = $count (expected 1)" }
  $text = $text.Replace($old, $new)
  [System.IO.File]::WriteAllText($src, $text, (New-Object System.Text.UTF8Encoding($false)))
}

Copy-Item $src $bak -Force
Build-Dist

$mutations = @(
  @{
    name = "A: #glosa rendering disabled"
    old  = '  $("glosa").textContent = formatGlosa(snap.glosa);'
    new  = '  $("glosa").textContent = ""; // MUTATION A'
    spec = "07-glosa"
  },
  @{
    name = "B: onMapState does not update route line"
    old  = '  if (!Array.isArray(d.polyline) || !mapReady) return;'
    new  = '  if (!Array.isArray(d.polyline) || !mapReady) return; return; // MUTATION B'
    spec = "04-route"
  },
  @{
    name = "C: throw during page load"
    old  = '"use strict";'
    new  = '"use strict";' + "`n" + 'throw new Error("MUTATION C");'
    spec = "01-boot"
  },
  @{
    name = "D: PMTiles protocol registration removed"
    old  = '  let protocol;'
    new  = '  return; // MUTATION D' + "`n" + '  let protocol;'
    spec = "02-map"
  }
)

try {
  foreach ($m in $mutations) {
    Write-Host ""
    Write-Host "===== $($m.name) ====="
    Set-Mutation $m.old $m.new $m.name
    Build-Dist
    $bad = Invoke-E2E $m.spec
    Write-Host "  mutated  : exit=$($bad.code)  $($bad.tail)"

    Copy-Item $bak $src -Force
    Build-Dist
    $good = Invoke-E2E $m.spec
    Write-Host "  restored : exit=$($good.code)  $($good.tail)"

    $results += [PSCustomObject]@{
      Mutation     = $m.name
      Spec         = $m.spec
      MutatedExit  = $bad.code
      RestoredExit = $good.code
      Verdict      = if ($bad.code -ne 0 -and $good.code -eq 0) { "PASS" } else { "FAIL" }
    }
  }
} finally {
  Copy-Item $bak $src -Force
  Build-Dist
  $h1 = (Get-FileHash $src -Algorithm SHA256).Hash
  $h2 = (Get-FileHash $bak -Algorithm SHA256).Hash
  if ($h1 -ne $h2) {
    Write-Host "[restore] FATAL: app.js differs from backup after restore" -ForegroundColor Red
    exit 2
  }
  Write-Host ""
  Write-Host "[restore] app.js restored from backup (sha256 $($h1.Substring(0,16))) and dist rebuilt"
}

Write-Host ""
Write-Host "===== mutation validation summary ====="
$results | Format-Table -AutoSize
$failed = $results | Where-Object { $_.Verdict -ne "PASS" }
if ($failed) { Write-Host "MUTATION VALIDATION FAIL"; exit 1 }
Write-Host "MUTATION VALIDATION PASS"
exit 0
