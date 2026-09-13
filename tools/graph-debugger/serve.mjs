#!/usr/bin/env node
// graph-debugger 本地静态服务器（手工调试与冒烟门共用同一实现）。
//
// 为什么不用 `python -m http.server`（run-debugger.bat 的原指令）：
//   1. 它不实现 HTTP 字节服务。pmtiles 的 FetchSource 固定发送 Range 请求，收到
//      200 + 完整 Content-Length 的响应时直接抛「Server returned no content-length
//      header or content-length exceeding request. Check that your storage backend
//      supports HTTP Byte Serving.」，于是 map.pmtiles 覆盖层在手工调试路径下永远
//      加载不出来——而该覆盖层正是本页 P1-12 的功能。
//   2. 它的 MIME 表与 nav-server 不一致。本页的 MapLibre worker 以模块 worker 加载，
//      需要 JavaScript MIME（nav-server 只把 .js 映射为 text/javascript）。
// 手工路径与冒烟门复用本实现，避免「测试里能跑、手工打开不行」这类分叉。
//
// 用法：
//   node tools/graph-debugger/serve.mjs [端口]        缺省 127.0.0.1:8123
//   import { createStaticServer } from "./serve.mjs"  供 verify-graph-debugger.mjs 复用

import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * MIME 表。`.js` 必须是 JavaScript MIME（模块脚本受严格校验）；`.mjs` 同样按
 * JavaScript 处理——nav-server 会把 .mjs 落到 application/octet-stream，这正是
 * MapLibre worker 产物取名 .js 的原因，此处与之保持一致以免掩盖该约束。
 */
const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".geojson": "application/geo+json",
  ".pmtiles": "application/octet-stream",
  ".pbf": "application/x-protobuf",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".svg": "image/svg+xml",
  ".woff2": "font/woff2",
};

function contentType(path) {
  return MIME[extname(path).toLowerCase()] ?? "application/octet-stream";
}

/**
 * 解析单区间 Range 头。返回 {start, end} / {unsatisfiable:true} / null（无 Range）。
 * 只支持 `bytes=a-b`、`bytes=a-`、`bytes=-n` 三种形态：pmtiles 只用前两种，
 * 多区间不在需要之列。
 */
export function parseRange(header, size) {
  if (typeof header !== "string") return null;
  const m = /^bytes=(\d*)-(\d*)$/.exec(header.trim());
  if (!m || (m[1] === "" && m[2] === "")) return null;
  let start;
  let end;
  if (m[1] === "") {
    const suffix = Number(m[2]);
    if (suffix <= 0) return { unsatisfiable: true };
    start = Math.max(0, size - suffix);
    end = size - 1;
  } else {
    start = Number(m[1]);
    end = m[2] === "" ? size - 1 : Math.min(Number(m[2]), size - 1);
  }
  if (start >= size || start > end) return { unsatisfiable: true };
  return { start, end };
}

function send(res, status, headers, body, isHead) {
  res.writeHead(status, headers);
  if (isHead || body === undefined) res.end();
  else res.end(body);
}

/**
 * 建立只监听回环地址的静态服务器。
 *
 * @param {object} opts
 * @param {string} opts.root    静态根目录（相对路径按进程 cwd 解析）
 * @param {object} [opts.mounts] 额外挂载：URL 路径 → 磁盘绝对路径（用于把夹具
 *                              map.pmtiles 从仓库其它位置映射进来，不落盘到本目录）
 * @param {string} [opts.host]  监听地址，缺省 127.0.0.1
 * @param {number} [opts.port]  端口，缺省 0（由内核分配）
 */
export async function createStaticServer({ root, mounts = {}, host = "127.0.0.1", port = 0 } = {}) {
  const rootAbs = resolve(root);
  const mountMap = new Map(Object.entries(mounts).map(([k, v]) => [normalize(k), resolve(v)]));

  const server = createServer((req, res) => {
    handle(req, res, rootAbs, mountMap).catch((e) => {
      send(res, 500, { "Content-Type": "text/plain; charset=utf-8", "Cache-Control": "no-store" },
        `内部错误: ${e.message}`, req.method === "HEAD");
    });
  });

  await new Promise((ok, bad) => {
    server.once("error", bad);
    server.listen(port, host, ok);
  });

  const address = server.address();
  const origin = `http://${host}:${address.port}`;
  return {
    origin,
    host,
    port: address.port,
    root: rootAbs,
    url: (pathname = "/") => origin + (pathname.startsWith("/") ? pathname : `/${pathname}`),
    close: () => new Promise((ok) => server.close(() => ok())),
  };
}

async function handle(req, res, rootAbs, mountMap) {
  const isHead = req.method === "HEAD";
  if (req.method !== "GET" && !isHead) {
    send(res, 405, { "Content-Type": "text/plain; charset=utf-8", Allow: "GET, HEAD" }, "仅支持 GET/HEAD", isHead);
    return;
  }

  const url = new URL(req.url ?? "/", "http://localhost");
  let pathname;
  try {
    pathname = decodeURIComponent(url.pathname);
  } catch {
    send(res, 400, { "Content-Type": "text/plain; charset=utf-8" }, "路径解码失败", isHead);
    return;
  }
  if (pathname.endsWith("/")) pathname += "index.html";

  let abs = mountMap.get(normalize(pathname));
  if (!abs) {
    abs = resolve(rootAbs, pathname.replace(/^\/+/, ""));
    // 目录穿越防护：解析后的路径必须落在根目录之内。
    if (abs !== rootAbs && !abs.startsWith(rootAbs + sep)) {
      send(res, 403, { "Content-Type": "text/plain; charset=utf-8" }, "越界路径", isHead);
      return;
    }
  }

  let info;
  try {
    info = await stat(abs);
  } catch {
    send(res, 404, {
      "Content-Type": "text/plain; charset=utf-8",
      "Cache-Control": "no-store",
    }, "未找到", isHead);
    return;
  }
  if (!info.isFile()) {
    send(res, 403, { "Content-Type": "text/plain; charset=utf-8" }, "不是普通文件", isHead);
    return;
  }

  const baseHeaders = {
    "Content-Type": contentType(abs),
    "Cache-Control": "no-store",
    "Accept-Ranges": "bytes",
  };

  const range = parseRange(req.headers.range, info.size);
  if (range && range.unsatisfiable) {
    send(res, 416, { ...baseHeaders, "Content-Range": `bytes */${info.size}` }, undefined, isHead);
    return;
  }
  if (range) {
    const length = range.end - range.start + 1;
    res.writeHead(206, {
      ...baseHeaders,
      "Content-Range": `bytes ${range.start}-${range.end}/${info.size}`,
      "Content-Length": String(length),
    });
    if (isHead) res.end();
    else createReadStream(abs, { start: range.start, end: range.end }).pipe(res);
    return;
  }

  res.writeHead(200, { ...baseHeaders, "Content-Length": String(info.size) });
  if (isHead) res.end();
  else createReadStream(abs).pipe(res);
}

// ─── CLI ─────────────────────────────────────────────────────────────────────
const invokedDirectly = process.argv[1]
  && resolve(process.argv[1]).toLowerCase() === fileURLToPath(import.meta.url).toLowerCase();

if (invokedDirectly) {
  const root = resolve(fileURLToPath(import.meta.url), "..");
  const port = Number(process.argv[2] ?? 8123);
  if (!Number.isInteger(port) || port <= 0 || port > 65535) {
    console.error(`[graph-debugger] 端口非法：${process.argv[2]}`);
    process.exit(1);
  }
  if (!(await stat(resolve(root, "vendor", "maplibre-gl.js")).catch(() => null))) {
    console.error("[graph-debugger] 缺少 vendor/maplibre-gl.js——请先执行：");
    console.error("  cd tools/ets2nav-web && npm ci && npm run build:graph-debugger");
    process.exit(1);
  }
  if (!(await stat(resolve(root, "map.pmtiles")).catch(() => null))) {
    console.warn("[graph-debugger] 提示：无 map.pmtiles（矢量瓦片层）——仅显示图 GeoJSON");
  }
  const server = await createStaticServer({ root, port });
  console.log(`[graph-debugger] 已启动：${server.url("/")}（根目录 ${server.root}，仅回环）`);
  console.log("[graph-debugger] Ctrl+C 结束。");
}
