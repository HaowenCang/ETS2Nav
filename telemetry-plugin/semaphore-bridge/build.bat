@echo off
rem Build semaphore-bridge.dll (requires Visual Studio toolchain)
rem Usage: build.bat [Debug|Release]

setlocal
set CONFIG=%1
if "%CONFIG%"=="" set CONFIG=Release

set SDK_INC=%~dp0..\..\vendor\scs_sdk_1_14\include
set OUTDIR=%~dp0out

if not exist "%OUTDIR%" mkdir "%OUTDIR%"

call "E:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat" >nul
if errorlevel 1 (
    echo [ERROR] vcvars64.bat not found.
    exit /b 1
)

cl /nologo /O2 /EHsc /W3 /utf-8 ^
   /I "%SDK_INC%" ^
   /D WIN32 /D NDEBUG ^
   /LD ^
   "%~dp0semaphore-bridge.cpp" ^
   /Fe:"%OUTDIR%\semaphore-bridge.dll" ^
   /link /DEF:"%~dp0semaphore-bridge.def" /OUT:"%OUTDIR%\semaphore-bridge.dll"

if errorlevel 1 (
    echo [ERROR] Build failed
    exit /b 1
)
echo [OK] %OUTDIR%\semaphore-bridge.dll
endlocal
