@echo off
rem ---------------------------------------------------------------------------
rem Harness encoding precondition (NOT a test oracle).
rem
rem Why this exists: scripts\regression.ps1 contains Chinese semantic
rem assertions and log strings. Windows PowerShell 5.1 reads a .ps1 file
rem without a BOM as ANSI, which mis-decodes those literals: best case the
rem assertions can never match, worst case the tokenizer breaks with syntax
rem errors that have nothing to do with the real code. The file therefore has
rem to be UTF-8 with BOM, and this guard makes that contract machine-checked.
rem
rem This file is intentionally ASCII-only: cmd.exe reads .bat with the OEM
rem code page, so non-ASCII comments here would be mis-decoded as well.
rem
rem Returns: 0 = encoding OK; 4 = HARNESS FAILURE (encoding contract broken).
rem ---------------------------------------------------------------------------
setlocal
set "TARGET=%~dp0regression.ps1"
set "PS=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%PS%" set "PS=powershell.exe"
"%PS%" -NoProfile -ExecutionPolicy Bypass -Command "$p='%TARGET%'; if (-not (Test-Path -LiteralPath $p)) { Write-Host ('HARNESS FAILURE: missing ' + $p); exit 4 }; $b=Get-Content -LiteralPath $p -Encoding Byte -TotalCount 3; if ($b.Length -lt 3 -or $b[0] -ne 239 -or $b[1] -ne 187 -or $b[2] -ne 191) { Write-Host ('HARNESS FAILURE: ' + $p + ' is not UTF-8 with BOM.'); Write-Host '  Windows PowerShell 5.1 reads BOM-less .ps1 as ANSI, which corrupts its Chinese literals.'; Write-Host '  Re-save that file as UTF-8 with BOM.'; exit 4 }; exit 0"
set "RC=%ERRORLEVEL%"
endlocal & exit /b %RC%
