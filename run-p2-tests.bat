@echo off
rem P2 Full Regression (plan 162): P1 regression + cargo gates + dataset smoke + route regression + match replay + signal link + perf smoke
rem 2026-08-12: DATASET -> europe-v5 (P5 ferry-terminal fix generation); match trace self-generated when absent
rem             (was an implicit dependency on %TEMP%\real.navtrace - suite failed on a clean machine)
rem 2026-09-11: per-step verdicts decoupled from the cumulative FAIL flag. Previously every step printed
rem             "if %FAIL%==1 (echo ... FAIL)" so the FIRST failure made all LATER steps report FAIL even
rem             when their own command succeeded - a clippy error alone looked like a 6-step collapse and
rem             hid which steps were actually broken. FAIL now only feeds the final exit code.
setlocal
set ETS2_INSTALL=E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2
set DATASET=E:\Projects\Pi\ETS2Nav\data\europe-v5
set TRACE=%TEMP%\real.navtrace
set FAIL=0

echo === [0/7] match trace (self-generated if absent) ===
if not exist "%TRACE%" (
  cd nav-core
  cargo build --release -p nav-core-cli >nul 2>&1
  if errorlevel 1 (echo TRACE BUILD FAIL & set FAIL=1) else (
    target\release\nav-core-cli.exe syntrace -58456,32832:-52925,36510 %DATASET% "%TRACE%" >nul 2>&1
    if errorlevel 1 (echo TRACE GEN FAIL & set FAIL=1) else (echo TRACE GEN PASS)
  )
  cd ..
) else (
  echo TRACE EXISTS
)

echo === [1/7] P1 Regression Suite ===
call run-p1-tests.bat
if errorlevel 1 (echo P1 REGRESSION FAIL & set FAIL=1) else (echo P1 REGRESSION PASS)

echo === [2/7] cargo fmt/clippy/test ===
cd nav-core
cargo fmt --check >nul 2>&1
if errorlevel 1 (echo FMT FAIL & set FAIL=1) else (echo FMT PASS)
cargo clippy --all-targets 2>&1 | findstr /C:"warning" /C:"error" >nul
if errorlevel 1 (echo CLIPPY PASS) else (echo CLIPPY FAIL & set FAIL=1)
cargo test 2>&1 | findstr /C:"FAILED" >nul
if errorlevel 1 (echo CARGO TEST PASS) else (echo CARGO TEST FAIL & set FAIL=1)

echo === [3/7] dataset v2 smoke ===
tools\..\..\tools\dataset-reader-smoke\target\release\dataset-reader-smoke.exe %DATASET% >nul 2>&1
if errorlevel 1 (echo DATASET SMOKE FAIL & set FAIL=1) else (echo DATASET SMOKE PASS)

echo === [4/7] route regression ===
target\release\nav-core-cli.exe regression %DATASET% | findstr /C:"P2-18 Regression PASS" >nul
if errorlevel 1 (echo ROUTE REGRESSION FAIL & set FAIL=1) else (echo ROUTE REGRESSION PASS)

echo === [5/7] map-match replay ===
target\release\nav-core-cli.exe match "%TRACE%" %DATASET% | findstr /C:"HIGH" >nul
if errorlevel 1 (echo MATCH REPLAY FAIL & set FAIL=1) else (echo MATCH REPLAY PASS)

echo === [6/7] signal link ===
target\release\nav-core-cli.exe signal -58456,32832:-58456,35000 %DATASET% | findstr /C:"movement" >nul
if errorlevel 1 (echo SIGNAL LINK FAIL & set FAIL=1) else (echo SIGNAL LINK PASS)

echo === [7/7] performance smoke ===
target\release\nav-core-cli.exe bench %DATASET% | findstr /C:"Bench PASS" >nul
if errorlevel 1 (echo PERF SMOKE FAIL & set FAIL=1) else (echo PERF SMOKE PASS)

cd ..
if %FAIL%==1 (
  echo P2 Regression Suite: FAIL
  exit /b 1
)
echo P2 Regression Suite: ALL PASS
exit /b 0
