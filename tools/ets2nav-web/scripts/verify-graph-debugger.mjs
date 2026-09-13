#!/usr/bin/env node
// graph-debugger 浏览器冒烟门（P4R Batch 6B）。
//
// 目的：把「调试页不再依赖运行时公共 CDN 且自身脚本真的能执行」变成一条可独立运行的
// 判据，而不是一句代码走查结论。本门启动真实 headless Chromium（仓库锁定的
// @playwright/test / playwright，不用 jsdom 代替），从回环静态服务器打开
// tools/graph-debugger/index.html，并断言：
//
//   1. 页面导航成功（HTTP 200）；
//   2. 无未捕获异常（pageerror）；
//   3. 控制台无 error 级消息；
//   4. 无失败网络请求（requestfailed 与 4xx/5xx；仅豁免「可选资源不存在」探测的 404）；
//   5. 【去 CDN 的核心判据】全部 HTTP 请求都指向回环主机。此断言不依赖外网是否可达——
//      断网时 CDN 请求表现为加载失败，反而容易被误读成「没问题」，故必须在请求层观测；
//   6. MapLibre 由本地产物加载，且运行期自报版本等于 tools/ets2nav-web 的钉版、
//      不低于安全下限（GHSA-jrc7-96c5-q579 / CVE-2026-85061，影响 <= 6.4.0）；
//   7. worker 地址指向本地产物（MapLibre 6.x 打包成 IIFE 后的硬性接入点）；
//   8. 地图对象存在、style 已建立、图数据源已加入；
//   9. 页面自身脚本已执行（stats 元素被填充为图规模统计）；
//  10. Dijkstra 路径按钮可用并产出路径；
//  11. PMTiles 覆盖层（需要 map.pmtiles 夹具；缺失时明确标为 NOT VERIFIED，不静默通过）。
//
// 另有产物自证（不需浏览器，先跑）：vendor 的 maplibre 版本不低于下限、与
// tools/ets2nav-web 钉版一致，且 vendor 产物与 vendor/build-manifest.json 逐字节一致。
//
// 前置：先执行 `npm run build:graph-debugger`（vendor/ 是构建输出，不入库）。
//       vendor 缺失时本门以退出码 3 报「前置不可用」，不跳过、不记 PASS。
//
// 退出码：0 PASS；1 FAIL（任一断言不成立）；3 前置不可用（vendor 缺失 / 浏览器未安装 /
//        指定夹具不存在）；4 harness 故障（服务器、页面导航或本门自身异常）。
//
// 用法：
//   node scripts/verify-graph-debugger.mjs [--pmtiles <archive.pmtiles>] [--headed] [--keep]

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium } from "playwright";

import { createStaticServer } from "../../graph-debugger/serve.mjs";

const EXIT_PASS = 0;
const EXIT_FAIL = 1;
const EXIT_PRECONDITION = 3;
const EXIT_HARNESS = 4;

const HERE = dirname(fileURLToPath(import.meta.url)); // tools/ets2nav-web/scripts
const WEB = resolve(HERE, ".."); // tools/ets2nav-web
const REPO = resolve(WEB, "..", "..");
const DEBUGGER_DIR = join(REPO, "tools", "graph-debugger");
const VENDOR = join(DEBUGGER_DIR, "vendor");
const MANIFEST = join(VENDOR, "build-manifest.json");

/** vendor 必需产物：缺失即前置不成立（不得静默跳过）。 */
const REQUIRED_VENDOR = ["maplibre-gl.js", "maplibre-gl-worker.js", "maplibre-gl.css", "pmtiles.js"];

/** 安全下限自述常量，与 tools/ets2nav-web/scripts/verify-maplibre-version.mjs 同源。 */
const ADVISORY_FLOOR = "6.4.1";
const ADVISORY_ID = "GHSA-jrc7-96c5-q579";
const CVE_ID = "CVE-2026-85061";

/** 回环主机白名单：非网络协议（data: 等）不参与本判据。 */
const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost", "::1", "[::1]"]);

/**
 * 允许以 404 结束的「可选资源不存在」探测：这类响应表达的是资源缺失这一预期事实
 * （页面据此降级），其余任何 4xx/5xx 都算失败请求。
 */
const OPTIONAL_PROBE_PATHS = ["/map.pmtiles", "/favicon.ico", "/vendor/fonts/"];

const GRAPH_READY_TIMEOUT_MS = 60_000;

/** 中止信号：携带退出码，跳过断言汇总（未完成判定不得视为通过）。 */
class Abort extends Error {
  constructor(code, message) {
    super(message);
    this.code = code;
  }
}

const assertions = [];
const unverified = [];

let server = null;
let browser = null;
let context = null;

function check(name, ok, detail = "") {
  assertions.push({ name, ok: !!ok, detail });
  console.log(`[${ok ? "PASS" : "FAIL"}] ${name}${detail ? ` — ${detail}` : ""}`);
  return !!ok;
}

function skip(name, detail) {
  unverified.push({ name, detail });
  console.log(`[NOT VERIFIED] ${name} — ${detail}`);
}

function info(msg) {
  console.log(`[INFO] ${msg}`);
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function compareVersion(a, b) {
  const pa = String(a).split(".").map(Number);
  const pb = String(b).split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x !== y) return x < y ? -1 : 1;
  }
  return 0;
}

function hostOf(url) {
  try {
    return new URL(url).hostname;
  } catch {
    return null;
  }
}

function isHttp(url) {
  return /^https?:/i.test(url);
}

function isOptionalProbe(url, status) {
  if (status !== 404) return false;
  const pathname = pathOf(url);
  if (pathname === null) return false;
  return OPTIONAL_PROBE_PATHS.some((p) => (p.endsWith("/") ? pathname.startsWith(p) : pathname === p));
}

function pathOf(url) {
  try {
    return new URL(url).pathname;
  } catch {
    return null;
  }
}

/**
 * 控制台 404 日志的豁免判定。
 *
 * 浏览器对任何 404 都会记一条 console error（"Failed to load resource … 404"），
 * 而「可选资源不存在」是页面设计内的预期事实（页面据此降级并 console.warn），
 * 因此与请求级豁免共用同一份名单，并要求日志可定位到该路径——定位信息缺失时
 * 不予豁免，宁可报错也不放宽。
 */
function isOptionalProbeConsoleError(rec) {
  if (!/Failed to load resource/i.test(rec.text) || !/\b404\b/.test(rec.text)) return false;
  if (!rec.url) return false;
  return isOptionalProbe(rec.url, 404);
}

/** 前置：vendor 必需产物必须齐备，否则退出 3（不得静默跳过）。 */
function requireVendor() {
  const missing = REQUIRED_VENDOR.filter((f) => !existsSync(join(VENDOR, f)));
  if (!existsSync(MANIFEST)) missing.push("build-manifest.json");
  if (missing.length === 0) return;
  console.error(`[门] 前置不可用：vendor 产物缺失 —— ${missing.join(", ")}`);
  console.error("[门] 本门不跳过缺失产物，缺失即不可验证。生成方式：");
  console.error("[门]   cd tools/ets2nav-web && npm ci && npm run build:graph-debugger");
  throw new Abort(EXIT_PRECONDITION, "vendor 产物缺失");
}

/** 产物自证：manifest 记录、钉版一致性、产物哈希。不依赖浏览器，先跑。 */
function checkVendorProvenance() {
  const manifest = JSON.parse(readFileSync(MANIFEST, "utf8"));
  const webPkg = JSON.parse(readFileSync(join(WEB, "package.json"), "utf8"));
  const pin = webPkg?.dependencies?.["maplibre-gl"];
  const vendored = manifest?.dependencies?.["maplibre-gl"]?.version;

  info(`tools/ets2nav-web 钉版 maplibre-gl = ${pin}；vendor 记录版本 = ${vendored}`);
  let ok = true;
  ok = check("vendor 记录的 maplibre-gl 版本为三段式 semver",
    typeof vendored === "string" && /^\d+\.\d+\.\d+$/.test(vendored), `version=${vendored}`) && ok;
  ok = check(`vendor 的 maplibre-gl ${vendored} 不低于安全下限 ${ADVISORY_FLOOR}`,
    typeof vendored === "string" && compareVersion(vendored, ADVISORY_FLOOR) >= 0,
    `${ADVISORY_ID} / ${CVE_ID}：影响 <= 6.4.0，首个修复 ${ADVISORY_FLOOR}`) && ok;
  ok = check("vendor 版本与 tools/ets2nav-web 钉版一致（不存在第二份依赖副本）",
    vendored === pin, `vendor=${vendored} 钉版=${pin}`) && ok;

  const mismatched = [];
  const records = manifest.artifacts ?? {};
  for (const [rel, rec] of Object.entries(records)) {
    const abs = join(DEBUGGER_DIR, rel);
    if (!existsSync(abs)) {
      mismatched.push(`${rel}（缺失）`);
      continue;
    }
    if (sha256(abs) !== rec.sha256 || statSync(abs).size !== rec.bytes) {
      mismatched.push(`${rel}（哈希或长度与 manifest 不符）`);
    }
  }
  ok = check("vendor 产物与 manifest 记录逐字节一致（即当前构建）",
    Object.keys(records).length > 0 && mismatched.length === 0,
    mismatched.length ? mismatched.join("; ") : `${Object.keys(records).length} 项`) && ok;

  return { ok, manifest, pin, vendored, artifacts: records };
}

/** 定位 PMTiles 夹具：缺省复用 Web UI 构建产物，也可由 --pmtiles 显式给出。 */
function findFixture() {
  const idx = process.argv.indexOf("--pmtiles");
  if (idx > 0) {
    const given = process.argv[idx + 1];
    if (!given) {
      console.error("[门] --pmtiles 缺少路径参数");
      throw new Abort(EXIT_HARNESS, "--pmtiles 用法错误");
    }
    const abs = resolve(given);
    if (!existsSync(abs)) {
      console.error(`[门] 前置不可用：指定的 PMTiles 夹具不存在 —— ${abs}`);
      throw new Abort(EXIT_PRECONDITION, "指定的 PMTiles 夹具不存在");
    }
    return abs;
  }
  return [join(WEB, "dist", "map.pmtiles"), join(WEB, "map.pmtiles")].find((p) => existsSync(p)) ?? null;
}

async function launchChromium() {
  try {
    return await chromium.launch({
      headless: !process.argv.includes("--headed"),
      // --no-proxy-server：系统代理会接管回环请求，使「请求主机」观测失真；
      // 取径与 playwright.config.mjs 一致。
      args: ["--no-sandbox", "--disable-dev-shm-usage", "--no-proxy-server"],
    });
  } catch (e) {
    const msg = String(e && e.message ? e.message : e);
    if (/Executable doesn't exist|playwright install|browserType\.launch/i.test(msg)) {
      console.error(`[门] 前置不可用：Playwright Chromium 未安装或不可用 —— ${msg.split("\n")[0]}`);
      console.error("[门] 安装方式：cd tools/ets2nav-web && npx playwright install chromium");
      throw new Abort(EXIT_PRECONDITION, "Playwright Chromium 不可用");
    }
    throw e;
  }
}

async function main() {
  requireVendor();
  const provenance = checkVendorProvenance();
  if (!provenance.ok) info("产物自证未通过：后续浏览器断言仍会执行，但最终结论已为 FAIL。");

  const fixture = findFixture();
  if (fixture) info(`PMTiles 夹具：${fixture}（映射为 /map.pmtiles）`);
  else info("未提供 map.pmtiles 夹具：矢量瓦片层本次不覆盖（后续标为 NOT VERIFIED）");

  server = await createStaticServer({
    root: DEBUGGER_DIR,
    mounts: fixture ? { "/map.pmtiles": fixture } : {},
  });
  info(`静态服务器：${server.origin}（根目录 ${server.root}）`);

  browser = await launchChromium();
  context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await context.newPage();

  const requests = [];
  const failedRequests = [];
  const responses = [];
  const consoleErrors = [];
  const consoleWarnings = [];
  const pageErrors = [];

  page.on("request", (r) => requests.push({ url: r.url(), method: r.method(), type: r.resourceType() }));
  page.on("requestfailed", (r) => {
    const f = r.failure();
    failedRequests.push({ url: r.url(), method: r.method(), reason: f ? f.errorText : "unknown" });
  });
  page.on("response", (r) => responses.push({ url: r.url(), method: r.request().method(), status: r.status() }));
  page.on("console", (m) => {
    if (m.type() === "error") {
      const loc = m.location();
      consoleErrors.push({ text: m.text(), url: loc && loc.url ? loc.url : "" });
    } else if (m.type() === "warning") consoleWarnings.push(m.text());
  });
  page.on("pageerror", (e) => pageErrors.push(String(e && e.message ? e.message : e)));

  const pageUrl = server.url("/");
  let navigation = null;
  try {
    navigation = await page.goto(pageUrl, { waitUntil: "load", timeout: 30_000 });
  } catch (e) {
    throw new Abort(EXIT_HARNESS, `无法导航到 ${pageUrl}：${e.message}`);
  }

  // 页面自身脚本的终态：graph 与 pmtiles 都必须离开 pending。
  let settled = true;
  try {
    await page.waitForFunction(() => {
      const s = window.__graphDebugger && window.__graphDebugger.state;
      return !!s && s.graph !== "pending" && s.pmtiles !== "pending";
    }, undefined, { timeout: GRAPH_READY_TIMEOUT_MS });
  } catch {
    settled = false;
  }
  // 让瓦片/字体等延迟请求与随之而来的控制台消息落地，再做判定。
  await page.waitForTimeout(1500);

  const snapshot = await page.evaluate(() => {
    const d = window.__graphDebugger || {};
    const map = d.map;
    const ml = window.maplibregl;
    const statsEl = document.getElementById("stats");
    const dataNameEl = document.getElementById("data-name");
    let styleOk = false;
    try {
      styleOk = !!(map && typeof map.getStyle === "function" && map.getStyle());
    } catch {
      styleOk = false;
    }
    return {
      hasMaplibre: typeof ml === "object" && ml !== null,
      maplibreVersion: ml && typeof ml.getVersion === "function" ? ml.getVersion() : null,
      workerUrl: ml && typeof ml.getWorkerUrl === "function" ? ml.getWorkerUrl() : null,
      mapIsInstance: !!(map && ml && typeof ml.Map === "function" && map instanceof ml.Map),
      styleOk,
      hasEdgesSource: !!(map && typeof map.getSource === "function" && map.getSource("edges")),
      hasTilesSource: !!(map && typeof map.getSource === "function" && map.getSource("tiles")),
      hasRouteSource: !!(map && typeof map.getSource === "function" && map.getSource("route")),
      pmtilesLib: typeof window.pmtiles === "object" && window.pmtiles !== null
        && typeof window.pmtiles.Protocol === "function",
      stats: statsEl ? statsEl.textContent : null,
      dataName: dataNameEl ? dataNameEl.textContent : null,
      state: d.state ? { ...d.state } : null,
    };
  });

  // ─── 1. 导航成功 ───────────────────────────────────────────────────────────
  check("页面导航成功（HTTP 200）",
    !!navigation && navigation.status() === 200,
    navigation ? `${pageUrl} → HTTP ${navigation.status()}` : "无导航响应");

  // ─── 2~4. 控制台与网络错误 ─────────────────────────────────────────────────
  check("无未捕获异常（pageerror）", pageErrors.length === 0,
    pageErrors.length ? pageErrors.slice(0, 3).join(" | ") : "0 条");

  const realConsoleErrors = consoleErrors.filter((r) => !isOptionalProbeConsoleError(r));
  const optionalConsoleErrors = consoleErrors.length - realConsoleErrors.length;
  check("控制台无 error 级消息", realConsoleErrors.length === 0,
    realConsoleErrors.length
      ? realConsoleErrors.slice(0, 3).map((r) => `${r.text} @${r.url}`).join(" | ")
      : `0 条（其中 ${optionalConsoleErrors} 条为可选资源缺失探测的 404 日志`
        + `；warning ${consoleWarnings.length} 条）`);

  if (consoleWarnings.length > 0) {
    info(`控制台 warning（${consoleWarnings.length} 条）：${consoleWarnings.slice(0, 5).join(" | ")}`);
  }

  const httpResponses = responses.filter((r) => isHttp(r.url));
  const badResponses = httpResponses.filter((r) => r.status >= 400 && !isOptionalProbe(r.url, r.status));
  const optionalProbes = httpResponses.filter((r) => isOptionalProbe(r.url, r.status));
  check("无失败网络请求（requestfailed 与 4xx/5xx）",
    failedRequests.length === 0 && badResponses.length === 0,
    failedRequests.length || badResponses.length
      ? [...failedRequests.map((f) => `${f.method} ${f.url} (${f.reason})`),
        ...badResponses.map((r) => `${r.method} ${r.url} → ${r.status}`)].slice(0, 4).join(" | ")
      : `${httpResponses.length} 个响应全部 <400`
        + (optionalProbes.length ? `（其中 ${optionalProbes.length} 个为可选资源缺失探测 404）` : ""));

  // ─── 5. 去 CDN 的核心判据：请求主机必须全部是回环 ──────────────────────────
  const httpRequests = requests.filter((r) => isHttp(r.url));
  const nonLoopback = httpRequests.filter((r) => !LOOPBACK_HOSTS.has(hostOf(r.url)));
  const hosts = [...new Set(httpRequests.map((r) => hostOf(r.url)))].sort();
  check("零非回环请求（证明运行时公共 CDN 已移除）",
    nonLoopback.length === 0,
    nonLoopback.length
      ? `出现 ${nonLoopback.length} 个非回环请求：`
        + [...new Set(nonLoopback.map((r) => r.url))].slice(0, 4).join(" | ")
      : `请求主机集合 = {${hosts.join(", ")}}，共 ${httpRequests.length} 个请求`);
  info(`请求清单：${httpRequests.map((r) => `${r.type} ${r.method} ${new URL(r.url).pathname}`).join(" ; ")}`);

  // ─── 6. 本地 MapLibre + 运行期版本 ─────────────────────────────────────────
  const libResponse = httpResponses.find((r) => r.url.endsWith("/vendor/maplibre-gl.js"));
  check("MapLibre 由本地产物加载，运行期版本等于钉版且不低于安全下限",
    snapshot.hasMaplibre
      && snapshot.maplibreVersion === provenance.pin
      && compareVersion(snapshot.maplibreVersion ?? "0.0.0", ADVISORY_FLOOR) >= 0
      && !!libResponse && libResponse.status === 200,
    `maplibregl.getVersion()=${snapshot.maplibreVersion}（钉版 ${provenance.pin}，下限 ${ADVISORY_FLOOR}）`
      + `；vendor/maplibre-gl.js HTTP ${libResponse ? libResponse.status : "未请求"}`);

  // ─── 7. worker 指向本地产物 ────────────────────────────────────────────────
  const workerUrl = server.url("/vendor/maplibre-gl-worker.js");
  const workerResponse = httpResponses.find((r) => r.url.endsWith("/vendor/maplibre-gl-worker.js"));
  check("MapLibre worker 指向本地产物且已成功取回",
    snapshot.workerUrl === workerUrl && !!workerResponse && workerResponse.status === 200,
    `setWorkerUrl→${snapshot.workerUrl}；HTTP ${workerResponse ? workerResponse.status : "未请求"}`);

  // ─── 8. 地图对象与图源 ─────────────────────────────────────────────────────
  check("地图对象存在、style 已建立、图数据源已加入",
    snapshot.mapIsInstance && snapshot.styleOk && snapshot.hasEdgesSource,
    `instanceof maplibregl.Map=${snapshot.mapIsInstance}；style=${snapshot.styleOk}`
      + `；source(edges)=${snapshot.hasEdgesSource}；state=${JSON.stringify(snapshot.state)}`);

  // ─── 9. 页面自身脚本已执行 ─────────────────────────────────────────────────
  const statsRe = /^节点 (\d+) \/ 边 (\d+) \/ 要素 (\d+) \/ 错误点 (\d+)$/;
  const statsMatch = typeof snapshot.stats === "string" ? statsRe.exec(snapshot.stats.trim()) : null;
  check("页面自身脚本已执行（stats 已填充为图规模统计）",
    !!statsMatch && Number(statsMatch[2]) > 0 && settled,
    settled
      ? `stats="${snapshot.stats}"`
      : `等待页面终态超时（${GRAPH_READY_TIMEOUT_MS} ms）：stats="${snapshot.stats}"`
        + ` state=${JSON.stringify(snapshot.state)}`);

  // ─── 10. Dijkstra 路径 ─────────────────────────────────────────────────────
  const edgeUids = firstEdgeUids();
  if (!edgeUids) {
    skip("Dijkstra 路径可用（按钮产出路径与 route 图源）",
      "data.geojson 中未找到带 from/to 的边，无法构造起终点");
  } else {
    let routeInfo = null;
    let routeError = "";
    try {
      await page.fill("#from", edgeUids.from);
      await page.fill("#to", edgeUids.to);
      await page.click("#btn-route");
      await page.waitForFunction(() => /^路径 \d+ 条边/.test(
        (document.getElementById("route-info") || {}).textContent || ""), undefined, { timeout: 15_000 });
      routeInfo = await page.evaluate(() => document.getElementById("route-info").textContent);
    } catch (e) {
      routeError = String(e && e.message ? e.message : e).split("\n")[0];
    }
    const routeSourceOk = await page.evaluate(
      () => !!(window.__graphDebugger.map.getSource && window.__graphDebugger.map.getSource("route")));
    check("Dijkstra 路径可用（按钮产出路径与 route 图源）",
      /^路径 \d+ 条边，总长 [\d.]+m$/.test(String(routeInfo ?? "")) && routeSourceOk,
      routeInfo
        ? `route-info="${routeInfo}"；source(route)=${routeSourceOk}`
        : `未产出路径：${routeError || "超时"}`);
  }

  // ─── 11. PMTiles 覆盖层（夹具存在时才可判定）───────────────────────────────
  if (!fixture) {
    skip("PMTiles 覆盖层已接入（protocol 注册 + vector source + 图层 + 真实取数）",
      "未找到 map.pmtiles 夹具；可用 --pmtiles <archive.pmtiles> 指定后重跑");
  } else {
    // 关键判据是「协议真的取到了字节」：addSource({type:"vector", url:"pmtiles://…"})
    // 在 protocol 未注册时**不会抛错**，失败推迟到瓦片请求阶段（浏览器报
    // "URL scheme pmtiles is not supported"）。只看同步接线会漏判，故要求观测到
    // 经协议转发、落到 /map.pmtiles 的成功 HTTP 取数。
    const tileFetches = httpResponses.filter(
      (r) => r.method === "GET" && pathOf(r.url) === "/map.pmtiles" && r.status < 400);
    check("PMTiles 覆盖层已接入（protocol 注册 + vector source + 图层 + 真实取数）",
      snapshot.pmtilesLib && snapshot.state?.pmtiles === "ready" && snapshot.hasTilesSource
        && String(snapshot.dataName ?? "").includes("map.pmtiles")
        && tileFetches.length > 0,
      `pmtiles.Protocol=${snapshot.pmtilesLib}；state.pmtiles=${snapshot.state?.pmtiles}`
        + `；source(tiles)=${snapshot.hasTilesSource}；data-name="${snapshot.dataName}"`
        + `；协议取数 ${tileFetches.length} 次（HTTP ${tileFetches.map((r) => r.status).join("/") || "无"}）`);
  }

  if (process.argv.includes("--keep")) {
    info("--keep：保留浏览器与服务器 60 秒以便人工检查");
    await page.waitForTimeout(60_000);
  }
}

/** 读取 data.geojson 的首条边，取 from/to 作为 Dijkstra 的起终点输入。 */
function firstEdgeUids() {
  const path = join(DEBUGGER_DIR, "data.geojson");
  if (!existsSync(path)) return null;
  let fc;
  try {
    fc = JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null;
  }
  for (const f of fc.features ?? []) {
    const p = f.properties ?? {};
    if (typeof p.from === "string" && typeof p.to === "string" && p.from !== p.to) {
      return { from: p.from, to: p.to };
    }
  }
  return null;
}

let aborted = null;
try {
  await main();
} catch (e) {
  if (e instanceof Abort) aborted = e;
  else {
    console.error(`[门] harness 故障：${e && e.stack ? e.stack : e}`);
    aborted = new Abort(EXIT_HARNESS, "本门自身异常");
  }
} finally {
  if (context) await context.close().catch(() => {});
  if (browser) await browser.close().catch(() => {});
  if (server) await server.close().catch(() => {});
}

console.log("");
if (aborted) {
  console.log(`GRAPH-DEBUGGER SMOKE ABORT(${aborted.code}): ${aborted.message}`
    + ` —— 未完成判定，不得视为通过`);
  process.exitCode = aborted.code;
} else {
  const failed = assertions.filter((a) => !a.ok);
  if (failed.length > 0) {
    console.log(`GRAPH-DEBUGGER SMOKE FAIL: ${failed.length}/${assertions.length} 项断言不成立`
      + ` — ${failed.map((f) => f.name).join("; ")}`);
    process.exitCode = EXIT_FAIL;
  } else if (assertions.length === 0) {
    console.log("GRAPH-DEBUGGER SMOKE FAIL: 未执行任何断言（不得视为通过）");
    process.exitCode = EXIT_HARNESS;
  } else {
    console.log(`GRAPH-DEBUGGER SMOKE PASS: ${assertions.length} 项断言全部成立，`
      + `${unverified.length} 项未验证`
      + (unverified.length ? `（${unverified.map((u) => u.name).join("; ")}）` : ""));
    process.exitCode = EXIT_PASS;
  }
}
