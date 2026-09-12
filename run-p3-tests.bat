@echo off
rem P3 Regression Suite (chain includes P2 -> P1). Thin wrapper: all test
rem judgement lives in scripts\regression.ps1 (single oracle). This file
rem contains no test logic; it only checks the encoding precondition, forwards
rem arguments, and propagates the PowerShell exit code verbatim.
rem Usage: run-p3-tests.bat [-Ets2Install "<game dir>"] [-Dataset "<dataset dir>"]
setlocal
call "%~dp0scripts\harness-encoding-guard.bat"
if errorlevel 1 exit /b 4
set "PS=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%PS%" set "PS=powershell.exe"
"%PS%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\regression.ps1" -Suite P3 %*
exit /b %ERRORLEVEL%
