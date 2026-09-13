@echo off
rem graph-debugger 启动脚本（P1 §45 / P1-12）
rem 用法：1) 构建本页依赖 2) 生成数据 3) 运行本脚本 4) 浏览器打开 http://127.0.0.1:8123/
rem
rem 依赖构建（必须先做；运行期不访问公共 CDN，MapLibre 取 vendor\ 下本地产物）：
rem   cd tools\ets2nav-web
rem   npm ci
rem   npm run build:graph-debugger
rem
rem 生成数据（二选一或都用）：
rem   map-inspector --geojson tools\graph-debugger\data.geojson --sectors <sec列表>   （边/查询）
rem   map-inspector --tiles tools\graph-debugger\map.pmtiles --sectors <sec列表>     （矢量瓦片）
rem
rem 服务由 serve.mjs 提供：它实现 HTTP 字节服务（Range），vector 瓦片层需要该能力；
rem python -m http.server 不实现字节服务，pmtiles 覆盖层在它下面加载不出来。
cd /d "%~dp0"
if not exist vendor\maplibre-gl.js (
  echo 缺少 vendor\maplibre-gl.js（MapLibre 本地产物）— 先生成：
  echo   cd tools\ets2nav-web
  echo   npm ci
  echo   npm run build:graph-debugger
  exit /b 1
)
if not exist data.geojson (
  echo 缺少 data.geojson — 先用 map-inspector 生成：
  echo   map-inspector --geojson tools\graph-debugger\data.geojson
  exit /b 1
)
if not exist map.pmtiles (
  echo 提示：无 map.pmtiles（矢量瓦片层）— 可用 map-inspector --tiles 生成
)
node serve.mjs 8123
