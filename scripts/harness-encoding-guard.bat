@echo off
rem ---------------------------------------------------------------------------
rem Harness encoding precondition (NOT a test oracle).
rem
rem Why this exists: scripts\regression.ps1 and scripts\ci\prepare-dataset.ps1
rem contain Chinese log/assertion strings. Windows PowerShell 5.1 reads a .ps1
rem file without a BOM as ANSI, which mis-decodes those literals: best case the
rem assertions can never match, worst case the tokenizer breaks with syntax
rem errors that have nothing to do with the real code. Those files therefore
rem have to be UTF-8 with BOM, and this guard makes that contract
rem machine-checked instead of relying on whoever edits them next.
rem
rem This file is intentionally ASCII-only: cmd.exe reads .bat with the OEM
rem code page, so non-ASCII comments here would be mis-decoded as well.
rem
rem Returns: 0 = encoding OK; 4 = HARNESS FAILURE (encoding contract broken).
rem ---------------------------------------------------------------------------
setlocal
set "PS=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%PS%" set "PS=powershell.exe"
"%PS%" -NoProfile -ExecutionPolicy Bypass -Command ^
  "$root='%~dp0';" ^
  "$targets=@('regression.ps1','ci\prepare-dataset.ps1');" ^
  "$bad=@();" ^
  "foreach ($t in $targets) {" ^
  "  $p=Join-Path $root $t;" ^
  "  if (-not (Test-Path -LiteralPath $p)) { Write-Host ('HARNESS FAILURE: missing ' + $p); exit 4 }" ^
  "  $b=Get-Content -LiteralPath $p -Encoding Byte -TotalCount 3;" ^
  "  if ($b.Length -lt 3 -or $b[0] -ne 239 -or $b[1] -ne 187 -or $b[2] -ne 191) { $bad += $t }" ^
  "}" ^
  "if ($bad.Count -gt 0) {" ^
  "  Write-Host ('HARNESS FAILURE: not UTF-8 with BOM: ' + ($bad -join ', '));" ^
  "  Write-Host '  Windows PowerShell 5.1 reads BOM-less .ps1 as ANSI, which corrupts its Chinese literals.';" ^
  "  Write-Host '  Re-save those files as UTF-8 with BOM.';" ^
  "  exit 4" ^
  "}" ^
  "Write-Host ('encoding guard: OK (' + ($targets -join ', ') + ')');" ^
  "exit 0"
set "RC=%ERRORLEVEL%"
endlocal & exit /b %RC%
