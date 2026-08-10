@echo off
rem graph-debugger 启动脚本（P1 §45 / P1-12）
rem 用法：1) 先生成数据 2) 运行本脚本 3) 浏览器打开 http://localhost:8123
rem 生成数据（二选一或都用）：
rem   map-inspector --geojson tools\graph-debugger\data.geojson --sectors <sec列表>   （边/查询）
rem   map-inspector --tiles tools\graph-debugger\map.pmtiles --sectors <sec列表>     （矢量瓦片）
cd /d "%~dp0"
if not exist data.geojson (
  echo 缺少 data.geojson — 先用 map-inspector 生成：
  echo   map-inspector --geojson tools\graph-debugger\data.geojson
  exit /b 1
)
if not exist map.pmtiles (
  echo 提示：无 map.pmtiles（矢量瓦片层）— 可用 map-inspector --tiles 生成
)
python -m http.server 8123
