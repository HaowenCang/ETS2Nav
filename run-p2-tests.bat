@echo off
rem P2 Regression Suite (chain includes P1). Thin wrapper: all test judgement
rem lives in scripts\regression.ps1 (single oracle). This file contains no test
rem logic; it only checks the encoding precondition, forwards arguments, and
rem propagates the PowerShell exit code verbatim.
rem Usage: run-p2-tests.bat [-Ets2Install "<game dir>"] [-Dataset "<dataset dir>"]
setlocal
call "%~dp0scripts\harness-encoding-guard.bat"
if errorlevel 1 exit /b 4
set "PS=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%PS%" set "PS=powershell.exe"
"%PS%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\regression.ps1" -Suite P2 %*
exit /b %ERRORLEVEL%
