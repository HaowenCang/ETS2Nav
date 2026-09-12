@echo off
rem P1 Regression Suite. Thin wrapper: all test judgement lives in
rem scripts\regression.ps1 (single oracle). This file contains no test logic;
rem it only checks the encoding precondition, forwards arguments, and
rem propagates the PowerShell exit code verbatim.
rem Usage: run-p1-tests.bat [-Ets2Install "<game dir>"] [other regression.ps1 args]
setlocal
call "%~dp0scripts\harness-encoding-guard.bat"
if errorlevel 1 exit /b 4
set "PS=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if not exist "%PS%" set "PS=powershell.exe"
"%PS%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\regression.ps1" -Suite P1 %*
exit /b %ERRORLEVEL%
