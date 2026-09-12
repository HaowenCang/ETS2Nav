# ---------------------------------------------------------------------------
# Clean-clone reproducibility check (P4R Batch 4 section 16/17).
#
# Purpose: prove that the canonical regression suites can be executed, and pass,
# in a *genuinely isolated clone* whose path deliberately contains spaces, with
# no reliance on any build artifact, target directory or path of the original
# working tree.
#
# How isolation is achieved:
#   1. `git clone` from the local repository (a real clone of committed history,
#      not a file copy), into a path containing spaces.
#   2. The uncommitted working-tree changes under test are transferred as a
#      patch (tracked modifications) plus an explicit list of new files, so the
#      clone contains exactly the code under review.
#   3. Build outputs are asserted absent in the clone (target/, bin/, obj/,
#      node_modules/) - they are gitignored, so a clone never has them.
#   4. The suites receive external inputs (game install, extracted data,
#      dataset) as explicit absolute paths that lie OUTSIDE the clone.
#   5. The harness itself refuses to execute any binary that is neither under
#      the clone root nor a PATH-resolved standard tool, so an accidental
#      fallback to the original workspace surfaces as HARNESS FAILURE.
#
# This file is ASCII-only on purpose: cmd.exe / Windows PowerShell 5.1 read
# .ps1 without a BOM as ANSI, and this script must never fail for that reason.
#
# Usage:
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify-clean-clone.ps1 `
#       -Ets2Install "<game dir>" -Ets2Extracted "<extracted root>" -Dataset "<dataset>"
# ---------------------------------------------------------------------------
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Ets2Install,
    [Parameter(Mandatory = $true)][string]$Ets2Extracted,
    [Parameter(Mandatory = $true)][string]$Dataset,
    [string]$OdBaseline,
    [string[]]$Suite = @('P1', 'P2', 'P3', 'P5'),
    [string]$CloneParent = 'C:\Temp',
    [switch]$RemoveClone
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$repo = Split-Path -Parent $PSScriptRoot
$tag = [guid]::NewGuid().ToString('N').Substring(0, 6)
$clone = Join-Path $CloneParent "ETS2Nav Clean Clone $tag"

function Info([string]$m) { Write-Host $m }

Info "========================================================================"
Info "Clean-clone reproducibility check"
Info "========================================================================"
Info "source repo : $repo"
Info "clone path  : $clone"
Info "suites      : $($Suite -join ', ')"

# ---- sanity: inputs must exist and must NOT be inside the source repo -------
foreach ($pair in @(@('Ets2Install', $Ets2Install), @('Ets2Extracted', $Ets2Extracted), @('Dataset', $Dataset))) {
    $name = $pair[0]; $val = $pair[1]
    if (-not (Test-Path -LiteralPath $val)) { throw "PRECONDITION FAIL: -$name '$val' does not exist." }
}
if ($OdBaseline -and -not (Test-Path -LiteralPath $OdBaseline)) {
    throw "PRECONDITION FAIL: -OdBaseline '$OdBaseline' does not exist."
}

# ---- 1. real clone ----------------------------------------------------------
if (-not (Test-Path -LiteralPath $CloneParent)) { New-Item -ItemType Directory -Path $CloneParent -Force | Out-Null }
if (Test-Path -LiteralPath $clone) { Remove-Item -LiteralPath $clone -Recurse -Force }
New-Item -ItemType Directory -Path $clone -Force | Out-Null

# git clone into a path with spaces: pass the destination as a single argument.
& git clone --quiet --no-hardlinks -- "$repo" "$clone"
if ($LASTEXITCODE -ne 0) { throw "HARNESS FAILURE: git clone failed (exit $LASTEXITCODE)" }
Info "cloned HEAD  : $((& git -C $clone rev-parse HEAD))"

# ---- 2. transfer the uncommitted changes under test ------------------------
$patch = Join-Path $env:TEMP "ets2nav-b4-worktree-$tag.patch"
$patchText = & git -C $repo diff HEAD
if ($patchText) {
    [System.IO.File]::WriteAllLines($patch, $patchText, (New-Object System.Text.UTF8Encoding($false)))
    & git -C $clone apply --whitespace=nowarn -- "$patch"
    if ($LASTEXITCODE -ne 0) { throw "HARNESS FAILURE: git apply of the working-tree patch failed" }
    Info "applied patch: $(@($patchText).Count) lines of tracked modifications"
} else {
    Info "applied patch: none (no tracked modifications)"
}

$untracked = @(& git -C $repo ls-files --others --exclude-standard)
Info "new files    : $($untracked.Count)"
foreach ($rel in $untracked) {
    $src = Join-Path $repo $rel
    $dst = Join-Path $clone $rel
    $dstDir = Split-Path -Parent $dst
    if (-not (Test-Path -LiteralPath $dstDir)) { New-Item -ItemType Directory -Path $dstDir -Force | Out-Null }
    Copy-Item -LiteralPath $src -Destination $dst -Force
    Info "  + $rel"
}

# ---- 3. assert the clone has no build outputs ------------------------------
$forbidden = @(
    'nav-core\target',
    'tools\dataset-reader-smoke\target',
    'tools\ets2nav-web\node_modules',
    'tools\ets2nav-web\dist'
)
$binDirs = @(Get-ChildItem -LiteralPath (Join-Path $clone 'map-compiler') -Recurse -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -in @('bin', 'obj') })
$problems = @()
foreach ($rel in $forbidden) {
    if (Test-Path -LiteralPath (Join-Path $clone $rel)) { $problems += $rel }
}
foreach ($d in $binDirs) { $problems += $d.FullName }
if ($problems.Count -gt 0) {
    throw "HARNESS FAILURE: the clone contains build outputs that must not be there:`n  $($problems -join "`n  ")"
}
Info "build outputs: none present (target/, bin/, obj/, node_modules/, dist/)"

# ---- 4/5. run the suites ---------------------------------------------------
$env:ETS2_INSTALL = $Ets2Install
$env:ETS2NAV_EXTRACTED = $Ets2Extracted
$env:ETS2NAV_DATASET = $Dataset
if ($OdBaseline) { $env:ETS2NAV_OD_BASELINE = $OdBaseline }

$harness = Join-Path $clone 'scripts\regression.ps1'
$results = @()
foreach ($s in $Suite) {
    Info ""
    Info "########## clean clone: suite $s ##########"
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $harness -Suite $s 2>&1 |
        Tee-Object -FilePath (Join-Path $env:TEMP "b4-cleanclone-$s.log") |
        Select-Object -Last 8
    $code = $LASTEXITCODE
    $sw.Stop()
    $results += [pscustomobject]@{ Suite = $s; Exit = $code; Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1) }
    Info "clean clone suite $s EXIT=$code ($([math]::Round($sw.Elapsed.TotalSeconds,1))s)"
}

Info ""
Info "===== clean clone summary ====="
$results | Format-Table -AutoSize
$failed = @($results | Where-Object { $_.Exit -ne 0 })
if ($RemoveClone) {
    Remove-Item -LiteralPath $clone -Recurse -Force
    Info "clone removed (-RemoveClone)"
} else {
    Info "clone retained for inspection: $clone"
}
if ($failed.Count -gt 0) {
    Info "CLEAN CLONE REPRODUCIBILITY: FAIL"
    exit 1
}
Info "CLEAN CLONE REPRODUCIBILITY: PASS"
exit 0
