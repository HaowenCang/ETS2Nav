@echo off
rem run-bench.bat：P0-D 性能基准记录（PresentMon + 自动提权）
rem 用法：右键"以管理员身份运行" run-bench.bat <标签> <秒数>
rem 例：run-bench.bat baseline 120   （对照组：无 ETS2Nav 插件）
rem     run-bench.bat withnav 120    （实验组：有 ETS2Nav 插件）

setlocal

rem ---- 自动请求管理员权限 ----
net session >nul 2>&1
if %errorlevel% neq 0 (
    powershell -Command "Start-Process '%~f0' -Verb RunAs -ArgumentList '%1 %2'"
    exit /b
)

set TAG=%1
if "%TAG%"=="" set TAG=bench
set DURATION=%2
if "%DURATION%"=="" set DURATION=120

set PM="C:\Program Files\Intel\PresentMon\PresentMonApplication\PresentMon.exe"
set OUTDIR=%~dp0data
if not exist "%OUTDIR%" mkdir "%OUTDIR%"
set OUTFILE=%OUTDIR%\%TAG%.csv

echo ============================================
echo  P0-D 基准记录：%TAG%  （%DURATION% 秒）
echo  目标进程：eurotrucks2.exe
echo  输出：%OUTFILE%
echo  请在游戏内保持固定路线/速度驾驶 %DURATION% 秒
echo  开始时间：%date% %time%
echo ============================================

%PM% --process-name eurotrucks2.exe --output_file "%OUTFILE%" --duration %DURATION%

echo 记录完成：%OUTFILE%
echo 按任意键退出...
pause >nul
endlocal
