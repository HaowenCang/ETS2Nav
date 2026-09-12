@echo off
rem run-bench.bat: P0-D performance benchmark recorder (PresentMon 2.5.1 console, auto-elevate)
rem Usage: right-click "Run as administrator" run-bench.bat <tag> <seconds>
rem   e.g. run-bench.bat baseline 120   (control: no ETS2Nav plugins)
rem        run-bench.bat withnav 120    (experiment: ETS2Nav plugins loaded)
rem
rem This file is intentionally ASCII-only: cmd.exe reads .bat with the OEM code
rem page, so non-ASCII comments and echoed text get mis-decoded (same reason as
rem scripts\harness-encoding-guard.bat). P4R Batch 5 also removed a hardcoded
rem developer install path for PresentMon (section 36).

setlocal

rem ---- auto-elevate ----
net session >nul 2>&1
if %errorlevel% neq 0 (
    powershell -Command "Start-Process '%~f0' -Verb RunAs -ArgumentList '%1 %2'"
    exit /b
)

set TAG=%1
if "%TAG%"=="" set TAG=bench
set DURATION=%2
if "%DURATION%"=="" set DURATION=120

rem PresentMon path: PRESENTMON env var first, then the usual install locations.
set "PM="
if defined PRESENTMON (
    if exist "%PRESENTMON%" (
        set "PM=%PRESENTMON%"
    ) else (
        echo [WARN] PRESENTMON="%PRESENTMON%" not found; falling back to auto-detection
    )
)
if not defined PM for %%p in (
    "%ProgramFiles%\Intel\PresentMon\PresentMonConsoleApplication\PresentMon-2.5.1-x64.exe"
    "%ProgramFiles%\Intel\PresentMon\PresentMonConsoleApplication\PresentMon.exe"
    "%ProgramFiles%\Intel\PresentMon\PresentMon.exe"
) do if not defined PM if exist %%p set "PM=%%~fp"
if not defined PM (
    echo [ERROR] PresentMon not found. Install it, or set PRESENTMON to the console exe path.
    exit /b 1
)

set OUTDIR=%~dp0data
if not exist "%OUTDIR%" mkdir "%OUTDIR%"
set OUTFILE=%OUTDIR%\%TAG%.csv

echo ============================================
echo  P0-D benchmark: %TAG%  (%DURATION% s)
echo  Target: eurotrucks2.exe
echo  Output: %OUTFILE%
echo  PresentMon: %PM%
echo  Drive the fixed route at steady speed for %DURATION% s
echo  Started: %date% %time%
echo ============================================

"%PM%" --process_name eurotrucks2.exe --output_file "%OUTFILE%" --timed %DURATION% --v1_metrics

echo Done: %OUTFILE%
echo Press any key to exit...
pause >nul
endlocal
