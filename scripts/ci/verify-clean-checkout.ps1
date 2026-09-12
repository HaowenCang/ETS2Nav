#Requires -Version 5.1
<#
.SYNOPSIS
  Assert that this checkout is genuinely pristine (P4R Batch 5 sections 35 and 36).

.DESCRIPTION
  Runs before anything else in every CI job. Two independent claims are checked:

    1. No build inputs or outputs are present. A GitHub checkout materialises only
       tracked files, so this proves the workflow itself has to produce every input
       (node_modules, dist, target, dataset). If a developer ever commits a build
       output "to help CI pass", this step fails instead of quietly succeeding.

    2. No tracked file looks like a build output. This is the repository-hygiene half:
       tracked *.obj/*.lib/*.exp/*.pdb, or tracked paths under bin/obj/target/dist/
       node_modules/, are release-packaging defects and must not reach a checkout.

  Intentionally ASCII-only: CI logs, and no need for the BOM contract that the
  Chinese-comment harness scripts carry.

  Exit codes: 0 = clean; 4 = HARNESS FAILURE (the checkout is not what CI assumes).
#>
[CmdletBinding()]
param(
    # Paths that must not exist. Directories holding build outputs or external assets.
    [string[]]$MustBeAbsent = @(
        'vendor',
        'data',
        'tools/ets2nav-web/vendor',
        'tools/ets2nav-web/node_modules',
        'tools/ets2nav-web/dist',
        'tools/ets2nav-web/test-results',
        'tools/ets2nav-web/playwright-report',
        'nav-core/target',
        'desktop/target',
        'tools/dataset-reader-smoke/target',
        'map-compiler/src/ScsHashFs/bin',
        'map-compiler/src/ScsHashFs/obj'
    )
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
Push-Location $root
try {
    Write-Host '=== clean checkout verification ==='
    $head = (& git rev-parse HEAD).Trim()
    Write-Host "repo root : $root"
    Write-Host "HEAD      : $head"

    $violations = @()
    foreach ($rel in $MustBeAbsent) {
        if (Test-Path -LiteralPath $rel) { $violations += "unexpected path present: $rel" }
    }
    if ($violations.Count -eq 0) {
        Write-Host "OK: none of the $($MustBeAbsent.Count) build-input/output paths exist"
    }

    # Tracked build outputs (repository hygiene, section 36).
    $tracked = & git ls-files
    $badArtifacts = @($tracked | Where-Object { $_ -match '\.(obj|lib|exp|pdb)$' })
    $badDirs = @($tracked | Where-Object { $_ -match '(^|/)(bin|obj|target|dist|node_modules)/' })
    if ($badArtifacts.Count -gt 0) { $violations += "tracked build artifacts: $($badArtifacts -join ', ')" }
    if ($badDirs.Count -gt 0) { $violations += "tracked files under build dirs: $($badDirs -join ', ')" }
    if ($badArtifacts.Count -eq 0 -and $badDirs.Count -eq 0) {
        Write-Host "OK: no tracked build outputs among $($tracked.Count) tracked files"
    }

    # The workflow itself must be present and tracked.
    if (-not (Test-Path -LiteralPath '.github/workflows/ci.yml' -PathType Leaf)) {
        $violations += '.github/workflows/ci.yml is missing'
    }

    if ($violations.Count -gt 0) {
        Write-Host ''
        Write-Host 'HARNESS FAILURE: checkout is not pristine:'
        foreach ($v in $violations) { Write-Host "  - $v" }
        exit 4
    }
    Write-Host 'clean checkout verified'
    exit 0
} finally {
    Pop-Location
}
