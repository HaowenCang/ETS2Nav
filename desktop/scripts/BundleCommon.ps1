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

# ── 相对路径推导（P4R Batch 6B §17 修复）─────────────────────────────────────
# 原实现是 `$FullPath.Substring($Root.Length)`，而 `$Root` 来自 `Resolve-Path ... .Path`、
# 子项来自 `Get-ChildItem` 的 `FileInfo.FullName`。两者**不保证是同一个规范化形式**：
# 在 GitHub runner 上 $env:TEMP 含 8.3 短名（`RUNNER~1`），而 FileInfo 返回长名
# （`runneradmin`），前缀长度因此不同，Substring 会静默切掉/多留字符，
# 得到一个**看起来像相对路径但指向别处**的字符串。
#
# 该缺陷在 Source Gates 上以 `Could not find a part of the path '...\est\neg\allowed.md'`
# 的形式暴露（`test` 被切掉了首字母 `t`），而不是在本地——本地两种形式恰好一致。
#
# 现在改为：根与子项都取自 `Get-Item` 的 `FullName`（同一规范化来源），并在相减之前
# **断言前缀关系**，不一致就显式失败，而不是继续构造一个错误的路径。
function Get-NormalisedRoot {
    param([Parameter(Mandatory)][string]$Path)
    return (Get-Item -LiteralPath $Path -Force).FullName.TrimEnd('\', '/')
}

function Get-RelativePathChecked {
    param(
        [Parameter(Mandatory)][string]$RootFull,
        [Parameter(Mandatory)][string]$FullPath
    )
    $prefix = $RootFull + [IO.Path]::DirectorySeparatorChar
    if (-not $FullPath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw ("HARNESS FAILURE: 无法为 '$FullPath' 推导相对于 '$RootFull' 的路径——" +
            "它不在该根之下，或两者来自不同的路径规范化形式。拒绝继续构造路径。")
    }
    return $FullPath.Substring($prefix.Length).Replace('\', '/')
}

function New-FileEntry {
    param(
        [Parameter(Mandatory)][string]$Root,
        [Parameter(Mandatory)][string]$FullPath
    )
    $rel = Get-RelativePathChecked -RootFull $Root -FullPath $FullPath
    $item = Get-Item -LiteralPath $FullPath
    [pscustomobject]@{
        Rel    = $rel
        Bytes  = [int64]$item.Length
        Sha256 = (Get-FileHash -LiteralPath $FullPath -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

function Get-TreeEntries {
    param([Parameter(Mandatory)][string]$Root)
    $full = Get-NormalisedRoot -Path $Root
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
