@echo off
rem P3 Regression Suite (P3-driving-assistant-plan.md P3-07): P2 regression + cargo gates + P3 speed lookahead smoke + camera verdict
setlocal
set ETS2_INSTALL=E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2
set DATASET=E:\Projects\Pi\ETS2Nav\data\europe-v4
set FAIL=0

echo === [1/4] P2 Regression Suite ===
call run-p2-tests.bat
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo P2 REGRESSION FAIL) else (echo P2 REGRESSION PASS)

echo === [2/4] cargo fmt/clippy/test ===
cd nav-core
cargo fmt --check >nul 2>&1
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo FMT FAIL) else (echo FMT PASS)
cargo clippy --all-targets 2>&1 | findstr /C:"warning" /C:"error" >nul
if errorlevel 1 (echo CLIPPY PASS) else (echo CLIPPY FAIL & set FAIL=1)
cargo test 2>&1 | findstr /C:"FAILED" >nul
if errorlevel 1 (echo CARGO TEST PASS) else (echo CARGO TEST FAIL & set FAIL=1)

echo === [3/4] P3 speed lookahead smoke ===
target\release\nav-core-cli.exe speed -58456,32832:-52925,36510 %DATASET% 3000 | findstr /C:"breaks=2" >nul
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo SPEED LOOKAHEAD FAIL) else (echo SPEED LOOKAHEAD PASS)

echo === [4/4] camera verdict smoke ===
cd ..\tools\camera-probe\CameraProbe
dotnet run -c Release 2>&1 | findstr /C:"VERDICT=NO-GO" >nul
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo CAMERA VERDICT FAIL) else (echo CAMERA VERDICT PASS)

cd ..\..\..
if %FAIL%==1 (
  echo P3 Regression Suite: FAIL
  exit /b 1
)
echo P3 Regression Suite: ALL PASS
exit /b 0
