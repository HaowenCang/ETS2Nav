@echo off
rem Build scs-nav-bridge.dll (requires Visual Studio toolchain)
rem Usage: build.bat [Debug|Release]

setlocal
set CONFIG=%1
if "%CONFIG%"=="" set CONFIG=Release

set SDK_INC=%~dp0..\..\vendor\scs_sdk_1_14\include
set OUTDIR=%~dp0out

if not exist "%OUTDIR%" mkdir "%OUTDIR%"

call "E:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
if errorlevel 1 (
    echo [ERROR] vcvars64.bat not found. Check VS installation path.
    exit /b 1
)

cl /nologo /O2 /EHsc /W3 /utf-8 ^
   /I "%SDK_INC%" ^
   /D WIN32 /D NDEBUG ^
   /LD ^
   "%~dp0scs-nav-bridge.cpp" ^
   /Fe:"%OUTDIR%\scs-nav-bridge.dll" ^
   /link /DEF:"%~dp0scs-nav-bridge.def" /OUT:"%OUTDIR%\scs-nav-bridge.dll"

if errorlevel 1 (
    echo [ERROR] Build failed
    exit /b 1
)
echo [OK] %OUTDIR%\scs-nav-bridge.dll
endlocal
