@echo off
rem graph-debugger 启动脚本（P1 §45）
rem 用法：1) 先生成数据（见下） 2) 运行本脚本 3) 浏览器打开 http://localhost:8123
rem 生成数据：map-inspector --geojson tools\graph-debugger\data.geojson --sectors <sec列表>
cd /d "%~dp0"
if not exist data.geojson (
  echo 缺少 data.geojson — 先用 map-inspector 生成：
  echo   map-inspector --geojson tools\graph-debugger\data.geojson
  exit /b 1
)
python -m http.server 8123
