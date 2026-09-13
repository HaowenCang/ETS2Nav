#Requires -Version 5.1
<#
.SYNOPSIS
  Machine-contract checks for the two tracked telemetry plugin DLLs (P4R Batch 6a).

.DESCRIPTION
  Release packaging needs the tracked binaries in telemetry-plugin/*/out/ to be
  checkable by a machine, offline and deterministically, without trusting a
  developer's description of how they were produced. This script asserts, for both
  DLLs:

    1. The file exists and is non-zero.
    2. It is a PE32+ x64 DLL. The headers are parsed in this process: the MZ
       signature, the PE\0\0 signature, the COFF Machine field (must be 0x8664),
       the IMAGE_FILE_DLL characteristic bit (0x2000) and the PE32+ optional
       header magic (0x20B). No external tool is invoked, so the check cannot
       depend on whether dumpbin happens to be on PATH.
    3. Every export named in the sibling .def file is present in the image's
       export directory, and nothing else is exported. The required names are
       read from the .def so the check cannot drift away from the build recipe,
       which uses /DEF and no __declspec(dllexport).
    4. The image contains no developer absolute path. Literal case-insensitive
       scans for E:\, C:\Users\, C:\Program Files, Projects\, the repository root,
       the user profile directory, plus the generic pattern [A-Za-z]:\ ; every
       match is reported together with its surrounding bytes.
    5. The provenance manifest corresponds to the artefact: the recorded SHA-256
       and size are compared against the file on disk, and the recorded build
       hashes are compared against the tracked binary to state plainly whether
       the tracked binary is the output of the recorded recipe.

  A condition that could not be evaluated is reported as NOT EVALUATED and counted
  as a violation: this script never prints PASS for something it did not check.

  Deterministic and offline: it reads only files in this repository and performs no
  network access.

  Exit codes: 0 = PASS; 2 = USAGE (the repository layout is not what this script
  assumes); 4 = ARTEFACT VIOLATION (at least one condition failed or was not
  evaluated).

  Intentionally ASCII-only, so no BOM contract is involved.
#>
[CmdletBinding()]
param(
    [string]$RepoRoot
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

$EXIT_PASS = 0
$EXIT_USAGE = 2
$EXIT_VIOLATION = 4

$ARTIFACTS = @(
    [pscustomobject]@{ Name = 'scs-nav-bridge';   Dir = 'telemetry-plugin/scs-nav-bridge' }
    [pscustomobject]@{ Name = 'semaphore-bridge'; Dir = 'telemetry-plugin/semaphore-bridge' }
)
$MANIFEST_REL = 'telemetry-plugin/scs-sdk-provenance.json'

$IMAGE_FILE_MACHINE_AMD64 = 0x8664
$IMAGE_FILE_DLL = 0x2000
$PE32_PLUS_MAGIC = 0x20B

# Caps so a pathological binary cannot flood the log; the count is always exact.
$MAX_CONTEXTS_PER_PATTERN = 6
$CONTEXT_BEFORE = 12
$CONTEXT_AFTER = 32

$script:Results = New-Object System.Collections.Generic.List[object]
$script:Notes = New-Object System.Collections.Generic.List[string]

function Write-Log { param([string]$Text = '') Write-Host $Text }

function Add-Result {
    param(
        [Parameter(Mandatory)][string]$Scope,
        [Parameter(Mandatory)][string]$Check,
        [Parameter(Mandatory)][ValidateSet('PASS', 'FAIL', 'NOT EVALUATED')][string]$Status,
        [string]$Observed = ''
    )
    $script:Results.Add([pscustomobject]@{ Scope = $Scope; Check = $Check; Status = $Status; Observed = $Observed })
    $colour = 'Gray'
    if ($Status -eq 'PASS') { $colour = 'Green' } elseif ($Status -eq 'FAIL') { $colour = 'Red' } else { $colour = 'Yellow' }
    Write-Host ("  [{0,-13}] {1,-50} {2}" -f $Status, $Check, $Observed) -ForegroundColor $colour
}

function Write-Note {
    param([string]$Text)
    $script:Notes.Add($Text)
    Write-Host "  note: $Text"
}

# -- Byte / PE helpers --------------------------------------------------------

function Read-U16 { param([Parameter(Mandatory)][byte[]]$B, [Parameter(Mandatory)][int]$Off) return [BitConverter]::ToUInt16($B, $Off) }
function Read-U32 { param([Parameter(Mandatory)][byte[]]$B, [Parameter(Mandatory)][int]$Off) return [BitConverter]::ToUInt32($B, $Off) }

function Get-AsciiZ {
    param([Parameter(Mandatory)][byte[]]$B, [Parameter(Mandatory)][int]$Off, [int]$Max = 512)
    if ($Off -lt 0 -or $Off -ge $B.Length) { return $null }
    $end = $Off
    $limit = [Math]::Min($B.Length, $Off + $Max)
    while ($end -lt $limit -and $B[$end] -ne 0) { $end++ }
    if ($end -ge $limit) { return $null }
    return [Text.Encoding]::ASCII.GetString($B, $Off, $end - $Off)
}

# Reads the pieces of the PE headers this contract cares about. Fields that could
# not be read are left at their sentinel so the caller can report them individually
# instead of collapsing everything into one opaque failure.
function Get-PeInfo {
    param([Parameter(Mandatory)][byte[]]$Bytes)
    $pe = [pscustomobject]@{
        HasMz               = $false
        Lfanew              = -1
        HasPeSignature      = $false
        Machine             = -1
        NumberOfSections    = -1
        TimeDateStamp       = [uint32]0
        Characteristics     = -1
        SizeOfOptionalHeader = -1
        OptionalMagic       = -1
        OptionalHeaderOff   = -1
        SectionTableOff     = -1
        ExportRva           = [uint32]0
        ExportSize          = [uint32]0
        DebugRva            = [uint32]0
        DebugSize           = [uint32]0
        Sections            = @()
    }
    if ($Bytes.Length -lt 0x40) { return $pe }
    if ($Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) { return $pe }
    $pe.HasMz = $true

    $lfanew = [BitConverter]::ToInt32($Bytes, 0x3C)
    $pe.Lfanew = $lfanew
    if ($lfanew -lt 0 -or ($lfanew + 24) -gt $Bytes.Length) { return $pe }
    if (-not ($Bytes[$lfanew] -eq 0x50 -and $Bytes[$lfanew + 1] -eq 0x45 -and
              $Bytes[$lfanew + 2] -eq 0x00 -and $Bytes[$lfanew + 3] -eq 0x00)) { return $pe }
    $pe.HasPeSignature = $true

    $coff = $lfanew + 4
    $pe.Machine = Read-U16 -B $Bytes -Off $coff
    $pe.NumberOfSections = Read-U16 -B $Bytes -Off ($coff + 2)
    $pe.TimeDateStamp = Read-U32 -B $Bytes -Off ($coff + 4)
    $pe.SizeOfOptionalHeader = Read-U16 -B $Bytes -Off ($coff + 16)
    $pe.Characteristics = Read-U16 -B $Bytes -Off ($coff + 18)

    $opt = $coff + 20
    $pe.OptionalHeaderOff = $opt
    if (($opt + 2) -le $Bytes.Length) { $pe.OptionalMagic = Read-U16 -B $Bytes -Off $opt }
    $pe.SectionTableOff = $opt + $pe.SizeOfOptionalHeader

    # PE32+ data directories start 112 bytes into the optional header.
    if ($pe.OptionalMagic -eq $PE32_PLUS_MAGIC -and ($opt + 112 + 16) -le $Bytes.Length) {
        $pe.ExportRva = Read-U32 -B $Bytes -Off ($opt + 112)
        $pe.ExportSize = Read-U32 -B $Bytes -Off ($opt + 116)
        $pe.DebugRva = Read-U32 -B $Bytes -Off ($opt + 112 + 48)
        $pe.DebugSize = Read-U32 -B $Bytes -Off ($opt + 112 + 52)
    }

    $sections = New-Object System.Collections.Generic.List[object]
    for ($i = 0; $i -lt $pe.NumberOfSections; $i++) {
        $s = $pe.SectionTableOff + ($i * 40)
        if (($s + 40) -gt $Bytes.Length) { break }
        $sections.Add([pscustomobject]@{
            Name             = ([Text.Encoding]::ASCII.GetString($Bytes, $s, 8)).TrimEnd([char]0)
            VirtualSize      = Read-U32 -B $Bytes -Off ($s + 8)
            VirtualAddress   = Read-U32 -B $Bytes -Off ($s + 12)
            SizeOfRawData    = Read-U32 -B $Bytes -Off ($s + 16)
            PointerToRawData = Read-U32 -B $Bytes -Off ($s + 20)
        })
    }
    $pe.Sections = $sections.ToArray()
    return $pe
}

function Convert-RvaToFileOffset {
    param([Parameter(Mandatory)]$Pe, [Parameter(Mandatory)][byte[]]$Bytes, [Parameter(Mandatory)][uint32]$Rva)
    foreach ($s in $Pe.Sections) {
        $span = [Math]::Max([int]$s.VirtualSize, [int]$s.SizeOfRawData)
        if ($Rva -ge $s.VirtualAddress -and $Rva -lt ($s.VirtualAddress + $span)) {
            $off = [int]$Rva - [int]$s.VirtualAddress + [int]$s.PointerToRawData
            if ($off -ge 0 -and $off -lt $Bytes.Length) { return $off }
        }
    }
    return -1
}

function Get-PeExports {
    param([Parameter(Mandatory)]$Pe, [Parameter(Mandatory)][byte[]]$Bytes)
    $res = [pscustomobject]@{ Ok = $false; ModuleName = $null; Names = @(); Reason = 'no export directory present' }
    if ($Pe.ExportRva -eq 0) { return $res }
    $expOff = Convert-RvaToFileOffset -Pe $Pe -Bytes $Bytes -Rva $Pe.ExportRva
    if ($expOff -lt 0) { $res.Reason = 'export directory RVA does not map into any section'; return $res }
    if (($expOff + 40) -gt $Bytes.Length) { $res.Reason = 'export directory header is truncated'; return $res }

    $nameRva = Read-U32 -B $Bytes -Off ($expOff + 12)
    $numNames = Read-U32 -B $Bytes -Off ($expOff + 24)
    $addrNames = Read-U32 -B $Bytes -Off ($expOff + 32)

    if ($nameRva -ne 0) {
        $o = Convert-RvaToFileOffset -Pe $Pe -Bytes $Bytes -Rva $nameRva
        if ($o -ge 0) { $res.ModuleName = Get-AsciiZ -B $Bytes -Off $o }
    }
    if ($numNames -eq 0) { $res.Reason = 'export directory declares zero named exports'; return $res }
    $namesOff = Convert-RvaToFileOffset -Pe $Pe -Bytes $Bytes -Rva $addrNames
    if ($namesOff -lt 0) { $res.Reason = 'export name pointer table does not map into any section'; return $res }

    $names = New-Object System.Collections.Generic.List[string]
    for ($i = 0; $i -lt $numNames; $i++) {
        $p = $namesOff + (4 * $i)
        if (($p + 4) -gt $Bytes.Length) { $res.Reason = 'export name pointer table is truncated'; return $res }
        $nr = Read-U32 -B $Bytes -Off $p
        $no = Convert-RvaToFileOffset -Pe $Pe -Bytes $Bytes -Rva $nr
        if ($no -lt 0) { continue }
        $nm = Get-AsciiZ -B $Bytes -Off $no
        if ($nm) { [void]$names.Add($nm) }
    }
    $res.Names = @($names | Sort-Object)
    $res.Ok = $true
    $res.Reason = ''
    return $res
}

# -- .def parsing -------------------------------------------------------------

function Get-DefExports {
    param([Parameter(Mandatory)][string]$Path)
    $names = New-Object System.Collections.Generic.List[string]
    $inExports = $false
    foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
        $t = $line.Trim()
        if ($t -eq '' -or $t.StartsWith(';')) { continue }
        $t = ($t -split ';')[0].Trim()
        if ($t -eq '') { continue }
        if ($t -match '^(?i)EXPORTS\b') {
            $inExports = $true
            $t = ($t -replace '^(?i)EXPORTS\s*', '').Trim()
            if ($t -eq '') { continue }
        }
        if (-not $inExports) { continue }
        $token = ($t -split '\s+')[0]
        if ($token -ne '' -and -not $names.Contains($token)) { [void]$names.Add($token) }
    }
    return @($names | Sort-Object)
}

function Get-DefLibraryName {
    param([Parameter(Mandatory)][string]$Path)
    foreach ($line in [System.IO.File]::ReadAllLines($Path)) {
        $t = $line.Trim()
        if ($t -match '^(?i)LIBRARY\b') {
            $rest = ($t -replace '^(?i)LIBRARY\s*', '').Trim()
            $rest = ($rest -split ';')[0].Trim()
            if ($rest -ne '') { return $rest }
        }
    }
    return $null
}

# -- binary scanning ----------------------------------------------------------

# Latin-1 maps each byte to the code point of the same value, so a match index in
# the string is also a byte offset. Case-insensitive matching then reduces to a
# regex over that string.
function Get-ByteMatches {
    param([Parameter(Mandatory)][string]$Latin1, [Parameter(Mandatory)][string]$Pattern, [switch]$AsRegex)
    if ($Pattern -eq '') { return @() }
    $rx = $Pattern
    if (-not $AsRegex) { $rx = [regex]::Escape($Pattern) }
    $opts = [System.Text.RegularExpressions.RegexOptions]::IgnoreCase -bor `
            [System.Text.RegularExpressions.RegexOptions]::CultureInvariant
    $hits = New-Object System.Collections.Generic.List[int]
    foreach ($m in [regex]::Matches($Latin1, $rx, $opts)) { [void]$hits.Add($m.Index) }
    return $hits.ToArray()
}

function Format-ByteContext {
    param([Parameter(Mandatory)][byte[]]$Bytes, [Parameter(Mandatory)][int]$Offset, [int]$Before = $CONTEXT_BEFORE, [int]$After = $CONTEXT_AFTER)
    $s = [Math]::Max(0, $Offset - $Before)
    $e = [Math]::Min($Bytes.Length - 1, $Offset + $After)
    $hex = (($Bytes[$s..$e]) | ForEach-Object { '{0:X2}' -f $_ }) -join ' '
    $asc = -join (($Bytes[$s..$e]) | ForEach-Object { if ($_ -ge 32 -and $_ -lt 127) { [char]$_ } else { '.' } })
    return [pscustomobject]@{ Start = $s; Hex = $hex; Ascii = $asc }
}

# -- setup --------------------------------------------------------------------

if (-not $RepoRoot) { $RepoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..')) }
$RepoRoot = [System.IO.Path]::GetFullPath($RepoRoot)
Set-Location -LiteralPath $RepoRoot

Write-Log '========================================================================'
Write-Log 'Telemetry plugin artefact contract verification (P4R Batch 6a)'
Write-Log '========================================================================'
Write-Log "repo root : $RepoRoot"
Write-Log "manifest  : $MANIFEST_REL"
Write-Log 'mode      : offline, no external tool invoked, headers parsed in-process'
Write-Log ''

# -- provenance manifest (loaded once; used by every artefact) ----------------
$manifest = $null
$manifestPath = Join-Path $RepoRoot ($MANIFEST_REL -replace '/', '\')
Write-Log '--- provenance manifest ---'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    Add-Result -Scope 'manifest' -Check 'provenance manifest is present' -Status 'FAIL' -Observed "missing: $MANIFEST_REL"
} else {
    try {
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        Add-Result -Scope 'manifest' -Check 'provenance manifest parses as JSON' -Status 'PASS' `
            -Observed "schema=$($manifest.schema)"
    } catch {
        Add-Result -Scope 'manifest' -Check 'provenance manifest parses as JSON' -Status 'FAIL' -Observed $_.Exception.Message
    }
}
if ($manifest) {
    Add-Result -Scope 'manifest' -Check 'manifest pins an SDK archive digest' -Status $(if ($manifest.sdk.sha256 -match '^[0-9a-f]{64}$') { 'PASS' } else { 'FAIL' }) `
        -Observed "sha256=$($manifest.sdk.sha256)"
    Add-Result -Scope 'manifest' -Check 'manifest records a redistribution conclusion' -Status $(if ($manifest.license.redistribution_conclusion) { 'PASS' } else { 'FAIL' }) `
        -Observed "spdx=$($manifest.license.spdx_identifier) permitted=$($manifest.license.redistribution_permitted_legally) committed=$($manifest.license.committed_to_repository)"
}
Write-Log ''

# -- scan patterns ------------------------------------------------------------
$scanSpecs = New-Object System.Collections.Generic.List[object]
$scanSpecs.Add([pscustomobject]@{ Id = 'drive-E';        Pattern = 'E:\';              AsRegex = $false })
$scanSpecs.Add([pscustomobject]@{ Id = 'users-profile';  Pattern = 'C:\Users\';        AsRegex = $false })
$scanSpecs.Add([pscustomobject]@{ Id = 'program-files';  Pattern = 'C:\Program Files'; AsRegex = $false })
$scanSpecs.Add([pscustomobject]@{ Id = 'projects';       Pattern = 'Projects\';        AsRegex = $false })
$scanSpecs.Add([pscustomobject]@{ Id = 'repo-root';      Pattern = $RepoRoot;          AsRegex = $false })
$scanSpecs.Add([pscustomobject]@{ Id = 'repo-root-fwd';  Pattern = ($RepoRoot -replace '\\', '/'); AsRegex = $false })
$userProfile = [Environment]::GetEnvironmentVariable('USERPROFILE')
$scanSpecs.Add([pscustomobject]@{ Id = 'userprofile-env'; Pattern = $(if ($userProfile) { $userProfile } else { '' }); AsRegex = $false })
$scanSpecs.Add([pscustomobject]@{ Id = 'any-drive-path'; Pattern = '[A-Za-z]:\\';       AsRegex = $true })

# -- per-artefact checks ------------------------------------------------------
foreach ($art in $ARTIFACTS) {
    $name = $art.Name
    $dllRel = "$($art.Dir)/out/$name.dll"
    $defRel = "$($art.Dir)/$name.def"
    $dllPath = Join-Path $RepoRoot ($dllRel -replace '/', '\')
    $defPath = Join-Path $RepoRoot ($defRel -replace '/', '\')

    Write-Log "--- artefact: $name ---"
    Write-Log "  path   : $dllRel"
    Write-Log "  recipe : $($art.Dir)/build.bat   def: $defRel"

    # 1. exists and non-zero
    $bytes = $null
    if (-not (Test-Path -LiteralPath $dllPath -PathType Leaf)) {
        Add-Result -Scope $name -Check 'file exists and is non-zero' -Status 'FAIL' -Observed "missing: $dllRel"
    } else {
        $len = (Get-Item -LiteralPath $dllPath).Length
        if ($len -le 0) {
            Add-Result -Scope $name -Check 'file exists and is non-zero' -Status 'FAIL' -Observed "size=$len bytes"
        } else {
            Add-Result -Scope $name -Check 'file exists and is non-zero' -Status 'PASS' -Observed "size=$len bytes"
            $bytes = [System.IO.File]::ReadAllBytes($dllPath)
        }
    }

    $dllSha = $null
    if ($bytes) {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try { $dllSha = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant() }
        finally { $sha.Dispose() }
        Write-Log "  sha256 : $dllSha"
    }

    $pe = $null
    $exports = $null
    if (-not $bytes) {
        foreach ($c in @('MZ signature', 'PE\0\0 signature', 'COFF Machine == 0x8664', 'IMAGE_FILE_DLL characteristic', 'PE32+ optional header magic', 'required exports present', 'export set matches the .def', 'no developer absolute path', 'manifest corresponds to the artefact')) {
            Add-Result -Scope $name -Check $c -Status 'NOT EVALUATED' -Observed 'the file is absent or empty'
        }
        Write-Log ''
        continue
    }

    # 2. PE structure
    $pe = Get-PeInfo -Bytes $bytes
    Add-Result -Scope $name -Check 'MZ signature' -Status $(if ($pe.HasMz) { 'PASS' } else { 'FAIL' }) `
        -Observed ("first 2 bytes = {0}" -f ((($bytes[0..([Math]::Min(1, $bytes.Length - 1))]) | ForEach-Object { '{0:X2}' -f $_ }) -join ' '))

    if (-not $pe.HasMz) {
        foreach ($c in @('PE\0\0 signature', 'COFF Machine == 0x8664', 'IMAGE_FILE_DLL characteristic', 'PE32+ optional header magic', 'required exports present', 'export set matches the .def', 'no developer absolute path', 'manifest corresponds to the artefact')) {
            Add-Result -Scope $name -Check $c -Status 'NOT EVALUATED' -Observed 'no MZ signature, PE headers not reachable'
        }
    } else {
        $peSigObserved = "e_lfanew=$($pe.Lfanew)"
        if ($pe.HasPeSignature) { $peSigObserved = "e_lfanew=0x$('{0:X}' -f $pe.Lfanew) bytes=50 45 00 00" }
        Add-Result -Scope $name -Check 'PE\0\0 signature' -Status $(if ($pe.HasPeSignature) { 'PASS' } else { 'FAIL' }) -Observed $peSigObserved

        if (-not $pe.HasPeSignature) {
            foreach ($c in @('COFF Machine == 0x8664', 'IMAGE_FILE_DLL characteristic', 'PE32+ optional header magic', 'required exports present', 'export set matches the .def', 'no developer absolute path', 'manifest corresponds to the artefact')) {
                Add-Result -Scope $name -Check $c -Status 'NOT EVALUATED' -Observed 'no PE signature, COFF header not reachable'
            }
        } else {
            Add-Result -Scope $name -Check 'COFF Machine == 0x8664' -Status $(if ($pe.Machine -eq $IMAGE_FILE_MACHINE_AMD64) { 'PASS' } else { 'FAIL' }) `
                -Observed ("Machine=0x{0:X4}" -f $pe.Machine)

            $isDll = (($pe.Characteristics -band $IMAGE_FILE_DLL) -eq $IMAGE_FILE_DLL)
            Add-Result -Scope $name -Check 'IMAGE_FILE_DLL characteristic' -Status $(if ($isDll) { 'PASS' } else { 'FAIL' }) `
                -Observed ("Characteristics=0x{0:X4} (DLL bit 0x2000 {1})" -f $pe.Characteristics, $(if ($isDll) { 'set' } else { 'clear' }))

            Add-Result -Scope $name -Check 'PE32+ optional header magic' -Status $(if ($pe.OptionalMagic -eq $PE32_PLUS_MAGIC) { 'PASS' } else { 'FAIL' }) `
                -Observed ("Magic=0x{0:X4} (0x20B is PE32+)" -f $pe.OptionalMagic)

            Write-Note ("$name headers: sections=$($pe.NumberOfSections) TimeDateStamp=0x{0:X8} exportDirRva=0x{1:X} size=$($pe.ExportSize) debugDirRva=0x{2:X} size=$($pe.DebugSize)" -f $pe.TimeDateStamp, $pe.ExportRva, $pe.DebugRva)

            # 3. exports
            if (-not (Test-Path -LiteralPath $defPath -PathType Leaf)) {
                Add-Result -Scope $name -Check 'required exports present' -Status 'NOT EVALUATED' -Observed "definition file missing: $defRel"
                Add-Result -Scope $name -Check 'export set matches the .def' -Status 'NOT EVALUATED' -Observed "definition file missing: $defRel"
            } else {
                $defExports = Get-DefExports -Path $defPath
                if ($defExports.Count -eq 0) {
                    Add-Result -Scope $name -Check 'required exports present' -Status 'NOT EVALUATED' -Observed "no EXPORTS section found in $defRel"
                    Add-Result -Scope $name -Check 'export set matches the .def' -Status 'NOT EVALUATED' -Observed "no EXPORTS section found in $defRel"
                } else {
                    $exports = Get-PeExports -Pe $pe -Bytes $bytes
                    if (-not $exports.Ok) {
                        Add-Result -Scope $name -Check 'required exports present' -Status 'NOT EVALUATED' -Observed "export table unreadable: $($exports.Reason)"
                        Add-Result -Scope $name -Check 'export set matches the .def' -Status 'NOT EVALUATED' -Observed "export table unreadable: $($exports.Reason)"
                    } else {
                        $missing = @($defExports | Where-Object { $exports.Names -notcontains $_ })
                        $extra = @($exports.Names | Where-Object { $defExports -notcontains $_ })
                        Add-Result -Scope $name -Check 'required exports present' -Status $(if ($missing.Count -eq 0) { 'PASS' } else { 'FAIL' }) `
                            -Observed ("required=[{0}] found={1}/{2} missing=[{3}]" -f ($defExports -join ','), ($defExports.Count - $missing.Count), $defExports.Count, ($missing -join ','))
                        Add-Result -Scope $name -Check 'export set matches the .def' -Status $(if ($extra.Count -eq 0) { 'PASS' } else { 'FAIL' }) `
                            -Observed ("image exports=[{0}] module={1} extra=[{2}]" -f ($exports.Names -join ','), $exports.ModuleName, ($extra -join ','))
                        $defLib = Get-DefLibraryName -Path $defPath
                        if ($defLib) {
                            $expectModule = $defLib
                            if ($expectModule -notmatch '(?i)\.dll$') { $expectModule = "$expectModule.dll" }
                            Add-Result -Scope $name -Check 'export module name matches the .def LIBRARY' -Status $(if ($exports.ModuleName -eq $expectModule) { 'PASS' } else { 'FAIL' }) `
                                -Observed ("image='$($exports.ModuleName)' def='$expectModule'")
                        } else {
                            Add-Result -Scope $name -Check 'export module name matches the .def LIBRARY' -Status 'NOT EVALUATED' -Observed "no LIBRARY line in $defRel"
                        }
                    }
                }
            }
        }
    }

    # 4. absolute path scan
    $latin1 = [Text.Encoding]::GetEncoding(28591).GetString($bytes)
    $totalHits = 0
    $unevaluated = New-Object System.Collections.Generic.List[string]
    $detailLines = New-Object System.Collections.Generic.List[string]
    foreach ($spec in $scanSpecs) {
        if (-not $spec.Pattern) {
            [void]$unevaluated.Add($spec.Id)
            $detailLines.Add("    $($spec.Id): NOT EVALUATED (pattern unavailable in this environment)")
            continue
        }
        $hits = @(Get-ByteMatches -Latin1 $latin1 -Pattern $spec.Pattern -AsRegex:$spec.AsRegex)
        $totalHits += $hits.Count
        if ($hits.Count -eq 0) {
            $detailLines.Add("    $($spec.Id): 0 matches for '$($spec.Pattern)'")
            continue
        }
        $shown = [Math]::Min($hits.Count, $MAX_CONTEXTS_PER_PATTERN)
        $detailLines.Add("    $($spec.Id): $($hits.Count) match(es) for '$($spec.Pattern)' (showing $shown)")
        for ($k = 0; $k -lt $shown; $k++) {
            $ctx = Format-ByteContext -Bytes $bytes -Offset $hits[$k]
            $detailLines.Add(("      @0x{0:X6} ({0})" -f $hits[$k]))
            $detailLines.Add("        hex: $($ctx.Hex)")
            $detailLines.Add("        asc: $($ctx.Ascii)")
        }
        if ($hits.Count -gt $shown) { $detailLines.Add("      ... $($hits.Count - $shown) further match(es) not printed") }
    }
    foreach ($line in $detailLines) { Write-Log $line }
    if ($unevaluated.Count -gt 0) {
        Add-Result -Scope $name -Check 'no developer absolute path' -Status 'NOT EVALUATED' `
            -Observed ("$totalHits match(es); pattern(s) not evaluated: " + ($unevaluated -join ','))
    } else {
        Add-Result -Scope $name -Check 'no developer absolute path' -Status $(if ($totalHits -eq 0) { 'PASS' } else { 'FAIL' }) `
            -Observed ("$totalHits match(es) across $($scanSpecs.Count) patterns")
    }

    # 5. manifest correspondence
    if (-not $manifest) {
        Add-Result -Scope $name -Check 'manifest corresponds to the artefact' -Status 'NOT EVALUATED' -Observed 'the provenance manifest is missing or unparseable'
        Add-Result -Scope $name -Check 'tracked binary is the recorded recipe output' -Status 'NOT EVALUATED' -Observed 'the provenance manifest is missing or unparseable'
    } else {
        $entry = $null
        foreach ($a in $manifest.artifacts) { if ($a.name -eq $name) { $entry = $a } }
        if (-not $entry) {
            Add-Result -Scope $name -Check 'manifest corresponds to the artefact' -Status 'FAIL' -Observed "the manifest has no artifacts entry named '$name'"
            Add-Result -Scope $name -Check 'tracked binary is the recorded recipe output' -Status 'NOT EVALUATED' -Observed 'no manifest entry'
        } else {
            $shaOk = ($entry.sha256 -eq $dllSha)
            $sizeOk = ([int]$entry.size_bytes -eq $bytes.Length)
            Add-Result -Scope $name -Check 'manifest corresponds to the artefact' -Status $(if ($shaOk -and $sizeOk) { 'PASS' } else { 'FAIL' }) `
                -Observed ("disk sha256=$dllSha size=$($bytes.Length) | manifest sha256=$($entry.sha256) size=$($entry.size_bytes)")

            $buildNode = $null
            $buildsProp = $manifest.reproducibility.builds.PSObject.Properties[$name]
            if ($buildsProp) { $buildNode = $buildsProp.Value }

            if (-not $buildNode) {
                Add-Result -Scope $name -Check 'tracked binary is the recorded recipe output' -Status 'NOT EVALUATED' -Observed "the manifest records no build hashes for '$name'"
            } else {
                $hA = $buildNode.A.sha256
                $hB = $buildNode.B.sha256
                $hC = $buildNode.C.sha256
                $threeEqual = ($hA -eq $hB) -and ($hB -eq $hC)
                Add-Result -Scope $name -Check 'recorded builds A/B/C are byte-identical' -Status $(if ($threeEqual) { 'PASS' } else { 'FAIL' }) `
                    -Observed "A=$hA B=$hB C=$hC"
                $trackedIsRecipe = $threeEqual -and ($hA -eq $dllSha)
                Add-Result -Scope $name -Check 'tracked binary is the recorded recipe output' -Status $(if ($trackedIsRecipe) { 'PASS' } else { 'FAIL' }) `
                    -Observed $(if ($trackedIsRecipe) { "tracked sha256 equals all three independent clean builds" } else { "tracked=$dllSha recorded recipe output=$hA - the tracked binary is NOT what the recorded recipe produces" })
            }
        }
    }
    Write-Log ''
}

# -- summary ------------------------------------------------------------------
$failed = @($script:Results | Where-Object { $_.Status -eq 'FAIL' })
$notEvaluated = @($script:Results | Where-Object { $_.Status -eq 'NOT EVALUATED' })
$passed = @($script:Results | Where-Object { $_.Status -eq 'PASS' })

Write-Log '========================================================================'
Write-Log ("summary: {0} PASS, {1} FAIL, {2} NOT EVALUATED (of {3} conditions)" -f $passed.Count, $failed.Count, $notEvaluated.Count, $script:Results.Count)
foreach ($r in $failed) { Write-Log "  FAIL          [$($r.Scope)] $($r.Check): $($r.Observed)" }
foreach ($r in $notEvaluated) { Write-Log "  NOT EVALUATED [$($r.Scope)] $($r.Check): $($r.Observed)" }
if ($failed.Count -eq 0 -and $notEvaluated.Count -eq 0) {
    Write-Log 'artefact contract verified'
    Write-Log 'exit 0 (0=PASS 2=USAGE 4=ARTEFACT VIOLATION)'
    Write-Log '========================================================================'
    exit $EXIT_PASS
}
Write-Log 'ARTEFACT VIOLATION: the tracked plugin artefacts do not satisfy the contract.'
Write-Log 'exit 4 (0=PASS 2=USAGE 4=ARTEFACT VIOLATION)'
Write-Log '========================================================================'
exit $EXIT_VIOLATION
