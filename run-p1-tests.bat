@echo off
rem P1-14 Regression Suite: one command -> complete P1 test suite
rem Usage: run-p1-tests.bat   (requires ETS2_INSTALL env var)
setlocal enabledelayedexpansion
cd /d "%~dp0"
set FAIL=0
if "%ETS2_INSTALL%"=="" (
  echo ERROR: set ETS2_INSTALL to the game directory first
  exit /b 1
)

echo ==========================================
echo  P1 Regression Suite
echo ==========================================

echo.
echo [1/6] unit tests
call dotnet test map-compiler\MapCompiler.sln -v q
if errorlevel 1 (echo   FAIL unit & set FAIL=1) else (echo   PASS)

echo.
echo [2/6] Berlin gate (semantic corpus + OD)
call tools\map-inspector\MapInspector\bin\Debug\net9.0\map-inspector.exe --install "%ETS2_INSTALL%" --sectors sec+0002-0002,sec+0002-0003,sec+0003-0002,sec+0003-0003,sec+0002-0001,sec+0002-0004,sec+0003-0001,sec+0003-0004 --gate
if errorlevel 1 (echo   FAIL berlin-gate & set FAIL=1) else (echo   PASS)

echo.
echo [3/6] Germany gate (scale)
call tools\map-inspector\MapInspector\bin\Debug\net9.0\map-inspector.exe --install "%ETS2_INSTALL%" --region germany --gate
if errorlevel 1 (echo   FAIL germany-gate & set FAIL=1) else (echo   PASS)

echo.
echo [4/6] determinism (two builds, compare routing.graph hash)
set DET1=%TEMP%\p1-det-1
set DET2=%TEMP%\p1-det-2
if exist %DET1% rmdir /s /q %DET1%
if exist %DET2% rmdir /s /q %DET2%
call tools\map-inspector\MapInspector\bin\Debug\net9.0\map-inspector.exe --install "%ETS2_INSTALL%" --sectors sec+0002-0002,sec+0002-0003 --dataset %DET1%
call tools\map-inspector\MapInspector\bin\Debug\net9.0\map-inspector.exe --install "%ETS2_INSTALL%" --sectors sec+0002-0002,sec+0002-0003 --dataset %DET2%
python -c "import hashlib,sys; files=['routing.graph','junction.graph','map.db','search.db']; h=lambda p: hashlib.sha256(open(p,'rb').read()).hexdigest(); a=[h(r'%DET1%\\'+f) for f in files]; b=[h(r'%DET2%\\'+f) for f in files]; print('  routing:', a[0][:16], 'junction:', a[1][:16]); sys.exit(0 if a==b else 1)"
if errorlevel 1 (echo   FAIL determinism & set FAIL=1) else (echo   PASS)

echo.
echo [5/6] Rust dataset reader
call tools\dataset-reader-smoke\target\release\dataset-reader-smoke.exe %DET1%
if errorlevel 1 (echo   FAIL dataset-reader & set FAIL=1) else (echo   PASS)

echo.
echo [6/6] Europe scale (failed_prefabs must be 0)
if exist %TEMP%\p1-europe rmdir /s /q %TEMP%\p1-europe
call tools\map-inspector\MapInspector\bin\Debug\net9.0\map-inspector.exe --install "%ETS2_INSTALL%" --all-sectors --dataset %TEMP%\p1-europe
if errorlevel 1 (echo   FAIL europe-scale (build) & set FAIL=1)
python -c "import json,sys; d=json.load(open(r'%TEMP%\\p1-europe\\diagnostics.json',encoding='utf-8')); n=len(d['failed_prefabs']); print('  failed_prefabs:', n); sys.exit(1 if n else 0)"
if errorlevel 1 (echo   FAIL europe-scale & set FAIL=1) else (echo   PASS)

echo.
echo ==========================================
if %FAIL%==0 (echo  P1 Regression Suite: ALL PASS) else (echo  P1 Regression Suite: FAIL)
echo ==========================================
exit /b %FAIL%
