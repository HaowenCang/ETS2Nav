@echo off
rem Build semaphore-bridge.dll (requires Visual Studio toolchain + SCS telemetry SDK headers)
rem Usage: build.bat [Debug|Release]
rem
rem P4R Batch 5 section 19: the Visual Studio location used to be hardcoded to one developer's
rem install path, which made this script unbuildable anywhere else and leaked that path
rem into a tracked file. It is now discovered through vswhere.
rem
rem The SCS telemetry SDK headers are NOT vendored in this repository (the include tree
rem lives under the gitignored vendor/ directory). Without them this script cannot build;
rem that limitation is recorded as a release-packaging blocker in
rem docs/validation/p4r-batch5-2026-09.md section 13 rather than worked around here.

setlocal
set CONFIG=%1
if "%CONFIG%"=="" set CONFIG=Release

set SDK_INC=%~dp0..\..\vendor\scs_sdk_1_14\include
set OUTDIR=%~dp0out

if not exist "%SDK_INC%\scssdk_telemetry.h" (
    echo [ERROR] SCS SDK headers not found at "%SDK_INC%".
    echo         The SCS telemetry SDK is not vendored in this repository.
    echo         See docs/validation/p4r-batch5-2026-09.md section 13.
    exit /b 1
)

if not exist "%OUTDIR%" mkdir "%OUTDIR%"

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" set "VSWHERE=%ProgramFiles%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (
    echo [ERROR] vswhere.exe not found. Install Visual Studio with the C++ workload.
    exit /b 1
)

set "VSPATH="
for /f "usebackq tokens=*" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VSPATH=%%i"
if not defined VSPATH (
    echo [ERROR] No Visual Studio installation with the MSVC x64 toolset was found.
    exit /b 1
)

call "%VSPATH%\VC\Auxiliary\Build\vcvars64.bat" >nul
if errorlevel 1 (
    echo [ERROR] vcvars64.bat failed: "%VSPATH%\VC\Auxiliary\Build\vcvars64.bat"
    exit /b 1
)

cl /nologo /O2 /EHsc /EHa /W3 /utf-8 ^
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
