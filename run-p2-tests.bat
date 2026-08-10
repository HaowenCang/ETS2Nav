@echo off
rem P2 Full Regression (plan 162): P1 regression + cargo gates + dataset smoke + route regression + match replay + signal link + perf smoke
setlocal
set ETS2_INSTALL=E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2
set DATASET=E:\Projects\Pi\ETS2Nav\data\europe-v4
set FAIL=0

echo === [1/7] P1 Regression Suite ===
call run-p1-tests.bat
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo P1 REGRESSION FAIL) else (echo P1 REGRESSION PASS)

echo === [2/7] cargo fmt/clippy/test ===
cd nav-core
cargo fmt --check >nul 2>&1
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo FMT FAIL) else (echo FMT PASS)
cargo clippy --all-targets 2>&1 | findstr /C:"warning" /C:"error" >nul
if errorlevel 1 (echo CLIPPY PASS) else (echo CLIPPY FAIL & set FAIL=1)
cargo test 2>&1 | findstr /C:"FAILED" >nul
if errorlevel 1 (echo CARGO TEST PASS) else (echo CARGO TEST FAIL & set FAIL=1)

echo === [3/7] dataset v2 smoke ===
tools\..\..\tools\dataset-reader-smoke\target\release\dataset-reader-smoke.exe %DATASET% >nul 2>&1
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo DATASET SMOKE FAIL) else (echo DATASET SMOKE PASS)

echo === [4/7] route regression ===
target\release\nav-core-cli.exe regression %DATASET% | findstr /C:"P2-18 Regression PASS" >nul
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo ROUTE REGRESSION FAIL) else (echo ROUTE REGRESSION PASS)

echo === [5/7] map-match replay ===
target\release\nav-core-cli.exe match C:\Users\20659\AppData\Local\Temp\real.navtrace %DATASET% | findstr /C:"HIGH" >nul
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo MATCH REPLAY FAIL) else (echo MATCH REPLAY PASS)

echo === [6/7] signal link ===
target\release\nav-core-cli.exe signal -58456,32832:-58456,35000 %DATASET% | findstr /C:"movement" >nul
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo SIGNAL LINK FAIL) else (echo SIGNAL LINK PASS)

echo === [7/7] performance smoke ===
target\release\nav-core-cli.exe bench %DATASET% | findstr /C:"Bench PASS" >nul
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo PERF SMOKE FAIL) else (echo PERF SMOKE PASS)

cd ..
if %FAIL%==1 (
  echo P2 Regression Suite: FAIL
  exit /b 1
)
echo P2 Regression Suite: ALL PASS
exit /b 0
