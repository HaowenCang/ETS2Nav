#Requires -Version 5.1
<#
.SYNOPSIS
  Shared helpers for the Desktop bundle tooling (P4R Batch 6A sections 6, 9, 10).

.DESCRIPTION
  The canonical tree digest implemented here MUST be byte-identical to
  `desktop/src/bundle.rs::tree_digest_lines`, because the Desktop verifies the
  bundle with the Rust implementation while the packaging scripts verify it with
  this one. The algorithm is:

    sort relative paths by ORDINAL order (case sensitive, forward slashes)
    for each entry append  "<rel>" NUL "<bytes>" NUL "<sha256>" LF
    SHA-256 the UTF-8 bytes of the concatenation

  Two details that are easy to get wrong and are therefore called out:
    * PowerShell's Sort-Object is culture aware; sorting with it would put
      "a/b" and "a_B" in a different order than Rust's byte-wise comparison for
      some inputs. [StringComparer]::Ordinal is used explicitly.
    * Encoding.UTF8.GetBytes does not emit a BOM, which is what we want; the
      preamble would change the digest.

  ASCII only on purpose: no BOM contract needed, and CI logs stay readable.
#>

Set-StrictMode -Version 2.0

function New-FileEntry {
    param(
        [Parameter(Mandatory)][string]$Root,
        [Parameter(Mandatory)][string]$FullPath
    )
    $rel = $FullPath.Substring($Root.Length).TrimStart('\', '/') -replace '\\', '/'
    $item = Get-Item -LiteralPath $FullPath
    [pscustomobject]@{
        Rel    = $rel
        Bytes  = [int64]$item.Length
        Sha256 = (Get-FileHash -LiteralPath $FullPath -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

function Get-TreeEntries {
    param([Parameter(Mandatory)][string]$Root)
    $full = (Resolve-Path -LiteralPath $Root).Path
    $out = New-Object System.Collections.ArrayList
    foreach ($f in (Get-ChildItem -LiteralPath $full -Recurse -File -Force)) {
        [void]$out.Add((New-FileEntry -Root $full -FullPath $f.FullName))
    }
    return , $out
}

function Get-TreeDigest {
    param([Parameter(Mandatory)]$Entries)
    $rels = @($Entries | ForEach-Object { $_.Rel })
    [Array]::Sort($rels, [System.StringComparer]::Ordinal)
    $byRel = @{}
    foreach ($e in $Entries) { $byRel[$e.Rel] = $e }
    $sb = New-Object System.Text.StringBuilder
    foreach ($r in $rels) {
        $e = $byRel[$r]
        [void]$sb.Append($r).Append([char]0).Append([string]$e.Bytes).Append([char]0).Append($e.Sha256).Append("`n")
    }
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($sb.ToString())
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try { $hash = $sha.ComputeHash($bytes) } finally { $sha.Dispose() }
    return (($hash | ForEach-Object { $_.ToString('x2') }) -join '')
}

function Get-TreeTotals {
    param([Parameter(Mandatory)]$Entries)
    $bytes = [int64]0
    foreach ($e in $Entries) { $bytes += $e.Bytes }
    return [pscustomobject]@{ Files = [int64]$Entries.Count; Bytes = $bytes }
}

function Write-JsonNoBom {
    param([Parameter(Mandatory)][string]$Path, [Parameter(Mandatory)]$Object, [int]$Depth = 12)
    $json = $Object | ConvertTo-Json -Depth $Depth
    [IO.File]::WriteAllText($Path, $json + "`n", (New-Object Text.UTF8Encoding($false)))
}

function Get-SourceCommit {
    param([Parameter(Mandatory)][string]$RepoRoot)
    Push-Location $RepoRoot
    try {
        $commit = (& git rev-parse HEAD).Trim()
        $dirty = @(& git status --porcelain).Count -gt 0
        return [pscustomobject]@{ Commit = $commit; Dirty = $dirty }
    } finally { Pop-Location }
}

# SHA-256 of one file as lowercase hex, plus its length.
function Get-FileIdentity {
    param([Parameter(Mandatory)][string]$Path)
    return [pscustomobject]@{
        Sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
        Bytes  = [int64](Get-Item -LiteralPath $Path).Length
    }
}
