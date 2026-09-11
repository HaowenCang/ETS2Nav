// E2E 公共工具（P4R Batch 2）。
//
// 诊断采集采用精确白名单：唯一允许的 console error 是「本地 glyphs 探测 404」——
// 该降级在 P4R Batch 1 已确立（跳过 city 文字层，几何图层照常）。任何其他 404、
// 任何 pageerror、任何应用自身的 console.error 都必须使测试失败。
// 明确不做「忽略所有 404」或「忽略所有 console.error」。

import { expect } from "@playwright/test";

/** 已知允许的地图资源降级：本地字体缺失导致的 404。 */
const GLYPH_URL_MARKER = "/vendor/fonts/";

export function isAllowedGlyphDegradation(msg) {
  if (msg.kind !== "console.error") return false;
  return typeof msg.url === "string" && msg.url.includes(GLYPH_URL_MARKER);
}

/**
 * 挂载诊断采集器。返回的对象在测试结束时交给 assertNoDiagnostics。
 * 不做任何宽泛过滤——白名单匹配在断言处集中判断并计数。
 */
export function collectDiagnostics(page) {
  const entries = [];
  const warnings = [];
  page.on("pageerror", (e) => entries.push({ kind: "pageerror", text: String(e), url: "" }));
  page.on("console", (m) => {
    if (m.type() === "error") {
      entries.push({ kind: "console.error", text: m.text(), url: m.location()?.url ?? "" });
    } else if (m.type() === "warning") {
      warnings.push({ kind: "console.warn", text: m.text(), url: m.location()?.url ?? "" });
    }
  });
  // ws 计数器由测试自行经 addInitScript 注入；此处仅保持字段形状稳定
  return { entries, warnings, ws: { created: 0, closed: 0 } };
}

/** 断言除白名单外的诊断为空。返回被放行的降级条目数，便于报告。 */
export function assertCleanDiagnostics(diag, label) {
  const allowed = diag.entries.filter(isAllowedGlyphDegradation);
  const bad = diag.entries.filter((e) => !isAllowedGlyphDegradation(e));
  if (bad.length) {
    const dump = bad.map((e) => `  [${e.kind}] ${e.text}${e.url ? ` @ ${e.url}` : ""}`).join("\n");
    throw new Error(`${label}: 出现 ${bad.length} 条非白名单诊断\n${dump}`);
  }
  return allowed.length;
}

/** 打开正式页面并等待 MapLibre style 加载完成。 */
export async function gotoApp(page, origin) {
  await page.goto(`${origin}/`, { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => map.loaded() === true, null, { timeout: 20_000 });
}

/** 等待 WS 连接建立（页面状态栏文案由 connect() 写入）。 */
export async function waitConnected(page) {
  await expect(page.locator("#conn-status")).toHaveClass(/on/, { timeout: 15_000 });
}

/** 通过真实 UI 设置目的地（填输入框 + 点按钮），返回 /api/route 的响应。 */
export async function setDestinationViaUi(page, { x, z }) {
  const resp = page.waitForResponse(
    (r) => r.url().includes("/api/route") && r.request().method() === "POST",
    { timeout: 20_000 },
  );
  await page.fill("#dest-x", String(x));
  await page.fill("#dest-z", String(z));
  await page.click("#btn-route");
  const r = await resp;
  expect(r.status()).toBe(200);
  return r;
}

/** 读取 MapLibre GeoJSON source 当前的坐标数组（经公开的 serialize()）。 */
export async function sourceCoordinates(page, sourceId) {
  return page.evaluate((id) => {
    const src = map.getSource(id);
    if (!src) return null;
    const data = src.serialize().data;
    if (!data) return null;
    return data.coordinates ?? null;
  }, sourceId);
}

/**
 * 把一条 §60 协议报文交给页面真实的 WS 消息处理入口。
 * 调用的是 app.js 顶层的 `dispatchMessage`（function 声明，即 window 属性），
 * 也就是 `ws.onmessage` 实际调用的同一个函数——因此 JSON 解析、类型分发、
 * DOM 渲染全部走生产代码，唯一未覆盖的是 socket 传输本身（由真实服务器的
 * 用例覆盖）。
 */
export async function deliverFrame(page, obj) {
  await page.evaluate((raw) => dispatchMessage(raw), JSON.stringify(obj));
}

/** 构造 §60 vehicle 帧（缺省字段可被 overrides 覆盖）。 */
export function vehicleFrame(overrides = {}) {
  return {
    type: "vehicle",
    state: "navigating",
    position: [-58400, 33000],
    speed_kmh: 50,
    map_limit_kmh: 50,
    remaining_m: 1234,
    remaining_s: 90,
    progress: 0.4,
    matched_edge: 1,
    next_maneuver: null,
    upcoming_signal: null,
    glosa: null,
    reminders: [],
    destination: "测试目标",
    diagnostics: "",
    ...overrides,
  };
}

/**
 * 断开页面的实时帧流，使后续的确定性注入不被 20 Hz 广播覆盖。
 *
 * 做法是把当前 socket 的处理器摘除后 close——`onclose` 被置空，因此不会触达
 * app.js 的重连排程（重连行为本身由 E2E-10 单独覆盖）。这只操作页面已有的公开
 * 全局，不改动生产代码语义。
 */
export async function detachStream(page) {
  await page.evaluate(() => {
    if (!ws) return;
    ws.onopen = ws.onmessage = ws.onerror = ws.onclose = null;
    try { ws.close(); } catch { /* 已关闭 */ }
  });
}

/** 等待 #glosa 文本满足给定形态；mode ∈ range | single | empty | any。 */
export async function waitGlosa(page, mode, timeout = 30_000) {
  await page.waitForFunction((m) => {
    const t = document.getElementById("glosa").textContent;
    if (m === "range") return /^建议 \d+–\d+ km\/h$/.test(t);
    if (m === "single") return /^建议 \d+ km\/h$/.test(t);
    if (m === "empty") return t === "";
    return true;
  }, mode, { timeout });
  return (await page.locator("#glosa").textContent()).trim();
}

/** 解析 "建议 A–B km/h" / "建议 A km/h"；非法格式返回 null。 */
export function parseGlosa(text) {
  let m = /^建议 (\d+)–(\d+) km\/h$/.exec(text);
  if (m) return { min: Number(m[1]), max: Number(m[2]) };
  m = /^建议 (\d+) km\/h$/.exec(text);
  if (m) return { min: Number(m[1]), max: Number(m[1]) };
  return null;
}
