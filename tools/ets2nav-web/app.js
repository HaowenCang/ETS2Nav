// ETS2Nav P4 正式 UI（§56-61）：WS 快照驱动 + 自动缩放（§57）+ 路由/设置
"use strict";

const $ = (id) => document.getElementById(id);
// §59 坐标转换（渲染层）：ETS2 米制世界 → 伪经纬度（与瓦片生成 lng=x/111320 一致）
const K = 111320;
const toLngLat = ([x, z]) => [x / K, z / K];
const map = new maplibregl.Map({
  container: "map",
  style: {
    version: 8,
    sources: {},
    layers: [
      { id: "bg", type: "background", paint: { "background-color": "#10141a" } },
    ],
    // 离线约束（P4R-01）：glyphs 只能指向同源本地路径，不得指向运行时公共 CDN。
    // 本地字体缺失时仅跳过 city 文字层，分级处理见 loadTiles。
    glyphs: "vendor/fonts/{fontstack}/{range}.pbf",
  },
  center: toLngLat([-58400, 33000]),
  zoom: 10,
});

// ─── 诊断输出（P4R §2.2：禁止静默吞错，失败必须可观测）───────────────────────
const logInfo = (msg, detail) => console.info(`[ets2nav] ${msg}`, detail ?? "");
const logWarn = (msg, detail) => console.warn(`[ets2nav] ${msg}`, detail ?? "");
const logError = (msg, detail) => console.error(`[ets2nav] ${msg}`, detail ?? "");

// ─── 会话令牌（P4R Batch 3）─────────────────────────────────────────────────
// 会话令牌（P4R Batch 3.5）。
//
// 服务端**两种模式**（默认回环 / `--lan`）都要求动态 API 与 `/ws` 携带 256-bit
// 令牌；回环不再是免认证理由——浏览器可以被远程页面驱使去连 `127.0.0.1`，服务端
// 看到的对端同样是回环。因此本页面无论从哪个地址打开都必须持有令牌。
//
// 令牌的三个来源，优先级从高到低：
//   1. URL fragment（手机扫码进入；`#token=…`）——服务端不下发，只能由二维码携带；
//   2. sessionStorage（同一 tab 会话内的刷新/重连）；
//   3. `GET /api/bootstrap`——**仅当页面自身位于回环**（本机浏览器或 Tauri）时。
//      这是服务端唯一的令牌出口，且它自身又限制「必须来自回环对端」。
//      手机打开的是 LAN 地址，不属于回环，因此永远无法从服务端直接取令牌。
//
// 为什么用 fragment 而非 query：fragment（`#` 之后）不会随请求发送给 HTTP
// server，因而不进入访问日志；query 会。服务端也只在 `/ws` 接受 query 令牌，
// `/api/*` 一律要求 `Authorization: Bearer`。
//
// 令牌只存在于内存变量与 sessionStorage（当前 tab 会话），不使用 localStorage
// 长期保存；服务端每次进程启动重新生成，旧令牌自然失效。
const TOKEN_KEY = "ets2nav.sessionToken";
const TOKEN_RE = /^[0-9a-f]{64}$/;

/**
 * 解析 fragment 中的令牌。
 * 返回 {present, value}：present 表示 fragment 里确实出现了 token 参数
 * （无论是否合法），value 仅在形态合法时为字符串。
 * 区分二者是为了对「携带了畸形令牌」给出显式告警，而不是静默当作没有令牌。
 */
function readTokenFragment(hash) {
  if (!hash) return { present: false, value: null };
  const body = hash.startsWith("#") ? hash.slice(1) : hash;
  for (const part of body.split("&")) {
    const eq = part.indexOf("=");
    if (eq > 0 && part.slice(0, eq) === "token") {
      const v = part.slice(eq + 1);
      return { present: true, value: TOKEN_RE.test(v) ? v : null };
    }
  }
  return { present: false, value: null };
}

/** 用 replaceState 从地址栏移除 fragment（令牌留在截图/书签/历史里没有意义）。 */
function stripFragment() {
  try {
    history.replaceState(null, "", location.pathname + location.search);
  } catch (e) {
    logWarn("history.replaceState 失败，地址栏 fragment 未能移除", e);
  }
}

/** 令牌的持久化：内存 + sessionStorage（后者不可用时仅内存，功能不受影响）。 */
function setSessionToken(v) {
  sessionToken = v;
  try {
    sessionStorage.setItem(TOKEN_KEY, v);
  } catch (e) {
    // 隐私模式等场景下 sessionStorage 可能不可用：令牌仍留在内存中，
    // 本 tab 内一切功能正常，仅刷新后需重新扫码/重新引导。
    logWarn("sessionStorage 不可用，令牌仅保留在内存", e);
  }
}

/**
 * 从 fragment / sessionStorage 取初始令牌（同步部分）。
 *
 * 畸形 fragment 一律按「没有 fragment」处理：先告警再继续走后续来源，
 * 不静默当作有一个无效令牌（那会让页面在后续每一步都收到 401 而不知原因）。
 */
function initSessionToken() {
  const frag = readTokenFragment(location.hash);
  if (frag.present) {
    stripFragment(); // 无论令牌是否合法都先抹掉
    if (frag.value === null) {
      logWarn("URL fragment 中的 token 形态非法（期望 64 位小写十六进制），已忽略");
    } else {
      try {
        sessionStorage.setItem(TOKEN_KEY, frag.value);
      } catch (e) {
        logWarn("sessionStorage 不可用，令牌仅保留在内存", e);
      }
      return frag.value;
    }
  }
  try {
    return sessionStorage.getItem(TOKEN_KEY) || null;
  } catch (e) {
    return null;
  }
}

let sessionToken = initSessionToken();

/** 最近一次 bootstrap 结果（二维码渲染用；不参与鉴权判定）。 */
let lastBootstrap = null;

/**
 * 统一 API 请求入口：有会话令牌即附 `Authorization: Bearer`，无令牌则普通请求。
 * 所有动态 API 调用（route/snapshot/search/settings）都应经此函数，避免在每处
 * fetch 里重复认证逻辑。令牌不写日志。
 */
async function apiFetch(url, init = {}) {
  const headers = new Headers(init.headers || undefined);
  if (sessionToken) headers.set("Authorization", "Bearer " + sessionToken);
  return fetch(url, { ...init, headers });
}

/** 给 WS URL 附加当前会话令牌（幂等：已含 token 参数则不重复附加）。 */
function withToken(url, token) {
  if (!token) return url;
  if (/[?&]token=/.test(url)) return url;
  return url + (url.includes("?") ? "&" : "?") + "token=" + token;
}

async function headOk(url) {
  const r = await fetch(url, { method: "HEAD" });
  return r.ok;
}

const TILE_FONT_STACK = "Open Sans Regular";

// 瓦片层（P4R-02）：标准 PMTiles protocol 流程——
//   new pmtiles.Protocol() → maplibregl.addProtocol("pmtiles", protocol.tile)
//   → vector source 使用 pmtiles:// URL。
// MapLibre GL JS 4.7.1 没有内置 "pmtiles" source 类型（原实现直接写
// `{type:"pmtiles"}`，必然抛错后被 catch 吞掉，瓦片层在任何情况下都不会出现）。
// addSource 仍须在 style load 之后（A2c-M5）。
//
// 错误分级（两类情况不得混同）：
//   情况 A  map.pmtiles 不存在      → INFO，降级为无底图模式，route/vehicle 照常
//   情况 B  资源存在但接入/解析失败  → ERROR，显式报告，不得静默 fallback
async function loadTiles() {
  // ── 情况 A：资源不存在，允许降级
  let hasTiles;
  try {
    hasTiles = await headOk("map.pmtiles");
  } catch (e) {
    logWarn("map.pmtiles 探测请求失败——进入无底图模式", e);
    return;
  }
  if (!hasTiles) {
    logInfo("未提供 map.pmtiles——无底图模式（route/vehicle 正常渲染）");
    return;
  }

  // ── 资源存在：此后任何失败都属于情况 B，必须报告
  if (typeof pmtiles === "undefined" || typeof pmtiles.Protocol !== "function") {
    logError("pmtiles 库未加载：vendor/pmtiles.js 缺失或前端未构建（npm run build）");
    return;
  }
  let protocol;
  try {
    protocol = new pmtiles.Protocol();
    maplibregl.addProtocol("pmtiles", protocol.tile);
  } catch (e) {
    logError("PMTiles protocol 注册失败", e);
    return;
  }

  try {
    map.addSource("tiles", { type: "vector", url: "pmtiles://map.pmtiles" });
  } catch (e) {
    logError("PMTiles vector source 创建失败", e);
    return;
  }

  const geometryLayers = [
    { id: "tile-road", type: "line", source: "tiles", "source-layer": "road",
      paint: { "line-color": "#3b82f6", "line-width": 1.2, "line-opacity": 0.75 } },
    { id: "tile-junction", type: "circle", source: "tiles", "source-layer": "junction",
      paint: { "circle-color": "#f59e0b", "circle-radius": 2 } },
    { id: "tile-poi", type: "circle", source: "tiles", "source-layer": "poi",
      paint: { "circle-color": "#10b981", "circle-radius": 2.5 } },
  ];
  for (const layer of geometryLayers) {
    try {
      map.addLayer(layer);
    } catch (e) {
      logError(`矢量图层添加失败: ${layer.id}`, e);
    }
  }

  // city 文字层依赖 glyphs。本地字体缺失属「资源不存在」，可降级（跳过该层），
  // 与 PMTiles 接入错误不同级，因此只 WARN 不 ERROR。
  const glyphProbe = `vendor/fonts/${encodeURIComponent(TILE_FONT_STACK)}/0-255.pbf`;
  let hasGlyphs = false;
  try {
    hasGlyphs = await headOk(glyphProbe);
  } catch (e) {
    logWarn("本地 glyphs 探测请求失败", e);
  }
  if (!hasGlyphs) {
    logWarn(`本地字体缺失（${glyphProbe}）——跳过 city 文字层，几何图层照常`);
    return;
  }
  try {
    map.addLayer({ id: "tile-city", type: "symbol", source: "tiles", "source-layer": "city",
      layout: { "text-field": ["get", "name"], "text-size": 11, "text-font": [TILE_FONT_STACK] },
      paint: { "text-color": "#fff" } });
  } catch (e) {
    logError("city 文字图层添加失败", e);
  }
}

// 图层与数据源（style 加载完成后初始化）
let mapReady = false;
map.on("load", () => {
  loadTiles();
  map.addSource("vehicle", { type: "geojson", data: { type: "Point", coordinates: [0, 0] } });
  map.addLayer({ id: "vehicle-dot", type: "circle", source: "vehicle",
    paint: { "circle-radius": 7, "circle-color": "#4cc38a", "circle-stroke-color": "#0b0f14", "circle-stroke-width": 3 } });
  map.addSource("route-line", { type: "geojson", data: { type: "LineString", coordinates: [] } });
  map.addLayer({ id: "route-line-layer", type: "line", source: "route-line",
    paint: { "line-color": "#7db8ff", "line-width": 4, "line-opacity": 0.9 } });
  mapReady = true;
});

// §57 自动缩放：Z = f(v, D_maneuver, complexity)
// 规范语义（v0.2 §57）：高速→显示较远范围（zoom 值更小）；城市→显示附近道路
// （zoom 更大）；复杂路口→进入 Junction View（再放大一档）。
// 2026-08-12 修复：速度项原为 +log2(v/40)，与「高速显示较远范围」相反（速度越高越
// 放大），现改为负号使 zoom 随速度单调下降。
function autoZoom(snap) {
  const v = Math.max(snap.speed_kmh || 0, 10);
  const d = snap.next_maneuver ? Math.max(snap.next_maneuver.distance_m, 30) : 500;
  const complex = snap.next_maneuver && snap.next_maneuver.roundabout_exit != null ? 1.2 : 1.0;
  const z = 13 + Math.log2(Math.max(10, d)) * -0.55 - Math.log2(v / 40) * 0.5;
  return Math.max(9, Math.min(16.5, z * complex));
}

let following = true;
// §57「手动拖动后临时暂停 follow，随后自动恢复」——恢复条件：停顿 ≥ RESUME_MS 且车辆在行驶。
const FOLLOW_RESUME_MS = 8000;
const FOLLOW_RESUME_MIN_KMH = 5;
let followPausedAt = 0;

function pauseFollow() {
  if (!following) return;
  following = false;
  followPausedAt = Date.now();
  $("follow-hint").classList.remove("hidden");
}

function resumeFollow() {
  following = true;
  followPausedAt = 0;
  $("follow-hint").classList.add("hidden");
}
let lastPos = null;
let lastReminderKey = "";

// §38 GLOSA 显示（P4R Batch 2）：只消费服务端结构化字段 snap.glosa。
// 明确禁止从 reminder.text 反推速度、正则解析中文 TTS、或从显示字符串反算区间——
// 那会形成第二套算法，且与 §38 的低精度量化语义脱钩。
// 契约（server.rs snapshot_json）：
//   glosa 为 null            → 无建议
//   glosa = {min_kmh,max_kmh} → 区间（i16，5 km/h 量化，min ≤ max）
// 非法值（非数值/非有限/负数/min>max）一律拒绝显示并 WARN，
// 不由 UI 修正后端数学错误。
function formatGlosa(g) {
  if (g == null) return "";
  if (typeof g !== "object") {
    logWarn("glosa 字段类型非法（期望对象或 null）", g);
    return "";
  }
  const lo = g.min_kmh, hi = g.max_kmh;
  if (!Number.isFinite(lo) || !Number.isFinite(hi)) {
    logWarn("glosa 数值非有限，已拒绝显示", g);
    return "";
  }
  if (lo < 0 || hi < 0 || lo > hi) {
    logWarn(`glosa 区间非法（${lo}–${hi}），已拒绝显示`, g);
    return "";
  }
  const a = Math.round(lo), b = Math.round(hi);
  return a === b ? `建议 ${a} km/h` : `建议 ${a}–${b} km/h`;
}

function onSnapshot(snap) {
  // 状态
  const chip = $("state-chip");
  chip.textContent = snap.state.toUpperCase();
  chip.className = snap.state === "navigating" ? "navigating" : snap.state === "rerouting" ? "rerouting" : "";

  // §57 自动恢复：手动拖动/滚轮暂停 follow 后，停顿足够久且车辆在行驶则自动恢复
  if (!following && followPausedAt
      && Date.now() - followPausedAt >= FOLLOW_RESUME_MS
      && (snap.speed_kmh || 0) >= FOLLOW_RESUME_MIN_KMH) {
    resumeFollow();
  }

  // 速度/限速
  $("speed-val").textContent = Math.round(snap.speed_kmh);
  $("limit-val").textContent = snap.map_limit_kmh > 0 ? snap.map_limit_kmh : "–";

  // 下一转向
  const mc = $("maneuver-card");
  if (snap.next_maneuver && snap.state === "navigating") {
    mc.classList.remove("hidden");
    const t = snap.next_maneuver.type.replace(/^ManeuverType::/, "");
    $("maneuver-type").textContent = t;
    $("maneuver-dist").textContent = snap.next_maneuver.distance_m.toFixed(0) + " m";
    $("maneuver-exit").textContent = snap.next_maneuver.roundabout_exit != null
      ? "环岛出口 " + snap.next_maneuver.roundabout_exit : "";
  } else {
    mc.classList.add("hidden");
  }

  // 信号 + GLOSA
  // 信号消失时必须同时清理 signal state / remaining / GLOSA——卡片隐藏后残留文本
  // 会在下次显示时短暂闪回上一帧的值（P4R Batch 2 §5）。GLOSA 与信号独立判定：
  // 信号在但区间不可行（glosa=null）同样必须清空，不得保留上一帧建议。
  const sc = $("signal-card");
  const sig = snap.upcoming_signal;
  if (sig && sig.state) {
    sc.classList.remove("hidden");
    const st = sig.state.replace(/^LightState::/, "");
    $("signal-state").textContent = st;
    $("signal-state").className = st.toLowerCase();
    $("signal-remaining").textContent = sig.remaining_s != null
      ? "剩余 " + sig.remaining_s.toFixed(1) + "s" : "";
  } else {
    sc.classList.add("hidden");
    $("signal-state").textContent = "";
    $("signal-state").className = "";
    $("signal-remaining").textContent = "";
  }
  $("glosa").textContent = formatGlosa(snap.glosa);

  // 提醒播报（变化时显示 3s）
  if (snap.reminders && snap.reminders.length) {
    const key = snap.reminders.map(r => r.text).join("|");
    if (key !== lastReminderKey) {
      lastReminderKey = key;
      const bar = $("reminder-bar");
      bar.textContent = snap.reminders.map(r => r.text).join(" · ");
      bar.classList.remove("hidden");
      clearTimeout(bar._t);
      bar._t = setTimeout(() => bar.classList.add("hidden"), 3500);
    }
  }

  // 剩余路线
  $("route-remain").textContent = snap.remaining_m != null ? (snap.remaining_m / 1000).toFixed(1) + " km" : "–";
  $("route-dest").textContent = snap.destination || "未设目的地";
  if (snap.progress != null) $("route-progress-fill").style.width = (snap.progress * 100).toFixed(1) + "%";

  // 车辆位置 + 跟随（§57：手动拖动暂停，恢复后自动回 follow；style 就绪后操作地图）
  if (snap.position && snap.position.length === 2) {
    lastPos = snap.position;
    if (mapReady) {
      map.getSource("vehicle").setData({ type: "Point", coordinates: toLngLat([snap.position[0], snap.position[1]]) });
      if (following) {
        map.easeTo({ center: toLngLat([snap.position[0], snap.position[1]]), zoom: autoZoom(snap), duration: 200 });
        $("follow-hint").classList.add("hidden");
      }
    }
  }
}

// WS 消息分发（§60 契约：单一全量 vehicle 快照 + map_state 独立事件）
// 2026-08-12 修复：原实现把全部帧无条件交给 onSnapshot，map_state 帧无 state 字段
// 而抛错，被外层 try/catch 静默吞掉——即已约定的 map_state 事件实际从未被处理。
// 现按 type 分发：vehicle → onSnapshot；map_state → 路线几何更新；未知类型计数上报。
let unknownEventCount = 0;
function dispatchMessage(raw) {
  let d;
  try {
    d = JSON.parse(raw);
  } catch (e) {
    return;   // 坏帧：仅丢弃解析失败的报文
  }
  const type = d.type || "vehicle";
  if (type === "vehicle") { onSnapshot(d); return; }
  if (type === "map_state") { onMapState(d); return; }
  unknownEventCount++;
  console.warn("[ets2nav] 未知事件类型:", type, d);
}

// map_state：设目的地后服务端推送路线几何（polyline）一次
function onMapState(d) {
  if (!Array.isArray(d.polyline) || !mapReady) return;
  map.getSource("route-line").setData({
    type: "LineString",
    coordinates: d.polyline.map(toLngLat),
  });
}

// WS 连接
// §61 断线重连（P4R Batch 2）：PLAN-P3plus B5 的验收项「断线重连行为」在原实现中
// 并不存在——onclose 只把文案改成「已断开」，没有任何重连路径。此处补最小实现：
// 有界指数退避（上限 RECONNECT_MAX_MS）+ 手动「连接」按钮随时可用 + 不产生重复
// socket（每次 connect 先作废旧 socket，旧 socket 的事件按 isCurrent 丢弃）。
const RECONNECT_BASE_MS = 1000;
const RECONNECT_MAX_MS = 15000;
let ws = null;
let reconnectAttempts = 0;
let reconnectTimer = null;

function scheduleReconnect() {
  if (reconnectTimer !== null) return; // 已有排程，不重复
  const delay = Math.min(RECONNECT_BASE_MS * 2 ** reconnectAttempts, RECONNECT_MAX_MS);
  reconnectAttempts++;
  const secs = Math.round(delay / 1000);
  $("conn-status").textContent = `已断开（${secs}s 后重连）`;
  $("conn-status").className = "";
  logWarn(`WS 已断开，${secs}s 后重连（第 ${reconnectAttempts} 次）`);
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    // Batch 3.5：重连前先尝试刷新令牌。
    //
    // 令牌是**进程级**的：服务端重启后旧令牌必然失效。本机页面（回环）可以重新
    // 引导取回新令牌，因此具备自愈能力；LAN 手机端不能（服务端对非回环一律 403），
    // 只能重新扫码——这正是「令牌出口仅限回环」的必然结果，不是缺陷。
    // 刷新失败（服务端尚未起来）时保持旧令牌并照常尝试：重试循环本身会再试。
    refreshTokenForReconnect().finally(connect);
  }, delay);
}

/**
 * 重连前刷新令牌（仅回环页面有效）。
 *
 * 刷新不会把令牌导向别处：请求目标是同源/回环 authority，响应体只被本页读取；
 * 跨源页面既无法触发本函数，也读不到 bootstrap 的响应（服务端的 Origin 策略与
 * CORS 白名单共同保证这一点）。
 */
async function refreshTokenForReconnect() {
  try {
    const d = await fetchBootstrap();
    if (d && d.token !== sessionToken) {
      setSessionToken(d.token);
      logInfo("会话令牌已刷新（服务端重启后重新引导）");
    }
  } catch (e) {
    logWarn("重连前刷新令牌失败，沿用当前令牌", e);
  }
}

function connect() {
  if (reconnectTimer !== null) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  // `base` 是输入框里的地址，永不含令牌，因此可以安全地进入状态文案与日志；
  // 令牌在构造 socket 的那一刻附加，因此每次重连都会带上**当前**令牌
  // （不存在「首次连接有令牌、重连后丢失」的形态）。
  const base = $("server-url").value.trim();
  const url = withToken(base, sessionToken);
  const prev = ws;
  let sock;
  try {
    sock = new WebSocket(url);
  } catch (e) {
    // URL 非法等构造期异常：不得以未捕获异常终止页面脚本
    logError(`WebSocket 构造失败（地址：${base}）`, e);
    $("conn-status").textContent = "连接错误";
    $("conn-status").className = "";
    return;
  }
  ws = sock; // 先接管：旧 socket 之后触发的所有事件都不再是「当前连接」
  if (prev && prev !== sock) {
    try { prev.close(); } catch (e) { /* 旧连接作废失败不影响新连接 */ }
  }
  const isCurrent = () => ws === sock;
  $("conn-status").textContent = "连接中…";
  $("conn-status").className = "";
  sock.onopen = () => {
    if (!isCurrent()) return;
    reconnectAttempts = 0; // 连上即重置退避
    $("conn-status").textContent = "已连接 " + base;
    $("conn-status").className = "on";
    // 二维码地址必须由服务器提供实际局域网地址，不得用 location.hostname
    refreshLanQr();
  };
  sock.onmessage = (ev) => {
    if (!isCurrent()) return;
    try { dispatchMessage(ev.data); } catch (e) { console.error("[ets2nav] 帧处理异常:", e); }
  };
  sock.onclose = () => {
    if (!isCurrent()) return; // 已被新连接取代：不报状态、不排重连
    $("conn-status").textContent = "已断开";
    $("conn-status").className = "";
    scheduleReconnect();
  };
  sock.onerror = () => {
    if (!isCurrent()) return;
    // onerror 之后必然触发 onclose，重连排程统一由 onclose 负责，避免双份排程
    $("conn-status").textContent = "连接错误";
    $("conn-status").className = "";
  };
}

// 路由
async function setRoute() {
  const fx = parseFloat($("dest-x").value), fz = parseFloat($("dest-z").value);
  if (!isFinite(fx) || !isFinite(fz)) { alert("请输入终点 x/z 坐标"); return; }
  // 起点 = 车辆当前位置（无则用默认 Berlin 点）
  const [px, pz] = lastPos || [-58456, 32832];
  const base = httpBase();
  if (!base) { alert("连接地址无法解析，无法发送路由请求"); return; }
  // 经统一 helper：两种模式下都会携带 Authorization: Bearer
  const r = await apiFetch(base + "/api/route", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ from: [px, pz], to: [fx, fz] }),
  });
  if (!r.ok) { alert("路由失败: " + r.status); return; }
  const d = await r.json();
  if (mapReady) map.getSource("route-line").setData({ type: "LineString", coordinates: d.polyline.map(toLngLat) });
  // 视野覆盖全路线
  const bounds = new maplibregl.LngLatBounds();
  d.polyline.forEach(([x, z]) => bounds.extend(toLngLat([x, z])));
  map.fitBounds(bounds, { padding: 60, duration: 600 });
}

// 设置面板 / 跟随 / 拖动暂停
$("btn-settings").onclick = () => $("settings-panel").classList.toggle("hidden");
$("btn-connect").onclick = connect;
$("btn-route").onclick = setRoute;
$("btn-reset").onclick = () => {
  if (mapReady) map.getSource("route-line").setData({ type: "LineString", coordinates: [] });
  $("dest-x").value = ""; $("dest-z").value = "";
};
$("btn-follow").onclick = () => resumeFollow();
map.on("dragstart", pauseFollow);
map.on("wheel", pauseFollow);

// 连接地址推导与令牌引导（P4R Batch 2 引入同源推导；Batch 3 修正范围；
// Batch 3.5 引入会话引导）。
//
// 原实现只在 hostname 属于回环（127.0.0.1/localhost/[::1]）时才推导同源 WS 地址，
// 因此手机从 `http://192.168.x.x:<port>/` 打开页面时不会去连
// `ws://192.168.x.x:<port>/ws`，而是沿用 index.html 里硬编码的
// `ws://127.0.0.1:8123/ws`——手机上 127.0.0.1 指向手机自己，连接必然失败。
// 这是 PLAN-P3plus B5「移动端可用」的阻断项。
//
// 现改为：只要页面是由普通 HTTP(S) 提供的，就按同源推导，与主机是否回环无关。
const DESKTOP_HOSTS = ["tauri.localhost"];

/** 该主机名是否指向本机回环（判断本机 UI 还是手机 UI，以及能否经 bootstrap 取令牌）。 */
function isLoopbackHostname(h) {
  return h === "127.0.0.1" || h === "localhost" || h === "::1" || h === "[::1]";
}

/**
 * 同源 WS 地址（不含令牌，令牌由 connect() 在建立连接时附加）。
 *
 * 返回 null 的两种情形都是「页面不是由 nav-server 用 HTTP 提供的」，此时代保持
 * index.html 中的默认地址不变：
 *   - 非 http/https：`file://` 直接打开、Tauri 的 `tauri://` 自定义协议；
 *   - 桌面端主机：Windows 上 Tauri 2.11.5 的页面 origin 实测为
 *     `http://tauri.localhost`（见 docs/validation/p4r-batch3-2026-09.md）。
 *     它形如普通 HTTP 页面，但不是 nav-server 提供的，若按同源推导会去连
 *     `ws://tauri.localhost/ws` 并必然失败。
 */
function sameOriginWsUrl() {
  const { protocol, hostname, host } = location;
  if (protocol !== "http:" && protocol !== "https:") return null;
  if (DESKTOP_HOSTS.includes(hostname)) return null;
  return (protocol === "https:" ? "wss://" : "ws://") + host + "/ws";
}

/**
 * nav-server 的 HTTP 基址（bootstrap 与 /api/* 的请求目标）。
 *
 * 页面由 nav-server 提供时即同源；Tauri 页面由 WebView2 从
 * `http://tauri.localhost` 提供，nav-server 另在回环端口上，此时从连接地址
 * 输入框（`ws://127.0.0.1:8123/ws`）反推。推导失败返回 null。
 */
function httpBase() {
  if (sameOriginWsUrl()) return location.origin;
  const raw = $("server-url").value.trim();
  if (!/^wss?:\/\//.test(raw)) return null;
  return raw
    .replace(/^wss:\/\//, "https://")
    .replace(/^ws:\/\//, "http://")
    .replace(/\/ws(\?.*)?$/, "");
}

/**
 * 向服务端索取会话引导信息（含令牌）。
 *
 * 只在**基址主机是回环**时尝试：手机打开的是 LAN 地址，服务端必然 403，
 * 客户端这一侧的判断只是避免一次注定失败的请求，真正的门在服务端。
 * 返回解析后的 JSON，任何失败都返回 null（不抛异常、不静默替换成假令牌）。
 */
async function fetchBootstrap() {
  const base = httpBase();
  if (!base) return null;
  let host;
  try {
    host = new URL(base).hostname;
  } catch (e) {
    logWarn("连接地址无法解析，会话引导跳过", e);
    return null;
  }
  if (!isLoopbackHostname(host)) return null;
  let r;
  try {
    r = await fetch(base + "/api/bootstrap");
  } catch (e) {
    logWarn("会话引导请求失败", e);
    return null;
  }
  if (!r.ok) {
    logWarn(`会话引导返回 ${r.status}，动态 API 与实时帧流将不可用`);
    return null;
  }
  let d;
  try {
    d = await r.json();
  } catch (e) {
    logWarn("会话引导返回体不是合法 JSON", e);
    return null;
  }
  if (!d || typeof d.token !== "string" || !TOKEN_RE.test(d.token)) {
    // 形态校验同样在前端做一次：服务端契约被破坏时应当显式告警，
    // 而不是把一个畸形令牌一路带到 WS URL 上。
    logWarn("会话引导未返回合法令牌，已忽略");
    return null;
  }
  lastBootstrap = d;
  return d;
}

/**
 * 确保在建立连接前持有令牌。
 *
 * 顺序：fragment/sessionStorage（同步，已在 initSessionToken 中完成）
 *       → 回环 bootstrap（异步）。
 * 手机（LAN 地址）没有 fragment 时不会走到 bootstrap，因此没有令牌——
 * 这是刻意结果：手机端的令牌只能经二维码 fragment 交付。
 */
async function ensureSessionToken() {
  if (sessionToken) return sessionToken;
  const d = await fetchBootstrap();
  if (d) setSessionToken(d.token);
  return sessionToken;
}

// ─── LAN 二维码（P4R Batch 3）───────────────────────────────────────────────
// 原实现用 `location.hostname` 构造二维码地址，本机页面打开时得到
// `http://127.0.0.1:<port>/`——该二维码对手机无效（手机上的 127.0.0.1 指向手机
// 自己）。现改为向服务器索取实际 RFC1918 候选地址。
//
// 多网卡（Wi-Fi / 以太网 / VPN / Hyper-V / WSL）并存时不静默挑选：服务器返回
// 全部候选并标注网卡名，候选多于一个时由用户在下拉框中选择。
async function refreshLanQr() {
  const box = $("qr-box");
  const base = httpBase();
  let host = null;
  try {
    host = base ? new URL(base).hostname : null;
  } catch { /* 地址非法：按「非本机页面」处理，下面隐藏二维码区块 */ }
  if (!host || !isLoopbackHostname(host)) {
    // 手机端本身就是被扫码的一方，不需要二维码；且远端 bootstrap 必然被拒。
    box.classList.add("hidden");
    return;
  }
  const data = lastBootstrap ?? (await fetchBootstrap());
  if (!data) return;
  renderLanQr(data);
}

function renderLanQr(data) {
  const box = $("qr-box");
  const note = $("lan-note");
  const pick = $("lan-pick");
  const canvas = $("qr");
  box.classList.remove("hidden");

  const addrs = data && data.lan_enabled === true && Array.isArray(data.addresses)
    ? data.addresses : [];

  const showNote = (text) => {
    canvas.classList.add("hidden");
    pick.classList.add("hidden");
    canvas.removeAttribute("data-qr-target");
    $("qr-url").textContent = "";
    note.textContent = text;
    note.classList.remove("hidden");
  };

  if (!data || data.lan_enabled !== true) {
    showNote("局域网未启用——服务以 --lan 启动后可扫码连接手机");
    return;
  }
  if (addrs.length === 0) {
    showNote("未发现可用局域网地址——请确认已连接 Wi-Fi 或以太网");
    return;
  }

  note.classList.add("hidden");
  canvas.classList.remove("hidden");
  const sel = $("lan-address");
  const previous = sel.value;
  sel.innerHTML = "";
  for (const a of addrs) {
    const opt = document.createElement("option");
    opt.value = a.address;
    opt.textContent = a.interface ? `${a.address}（${a.interface}）` : a.address;
    sel.appendChild(opt);
  }
  // 服务器已排序并把首选标为 preferred；仅当用户先前的选择仍然存在时才保留它，
  // 否则回到服务器首选，不静默漂移到列表里的其他地址。
  const kept = addrs.find((a) => a.address === previous);
  const preferred = addrs.find((a) => a.preferred) || addrs[0];
  sel.value = (kept || preferred).address;
  pick.classList.toggle("hidden", addrs.length < 2);
  lastBootstrap = data;
  drawQr(data.token, sel.value, data.port);
}

/** 用户在多个候选地址间切换时重绘二维码。 */
$("lan-address").addEventListener("change", () => {
  if (!lastBootstrap) return;
  drawQr(lastBootstrap.token, $("lan-address").value, lastBootstrap.port);
});

/**
 * 生成二维码。
 *
 * 可见文本只显示地址（`http://<addr>:<port>/`）。完整含令牌 URL 写入
 * `canvas.dataset.qrTarget`——它是 DOM 的一部分（不是可见文本，但仍可被同源
 * 脚本读取），保留它的唯一目的是让 E2E 能断言二维码内容。令牌会出现在失败
 * 截图与 Playwright trace 里，这一点在报告中明确记录，不声称「令牌不进入 DOM」。
 */
function drawQr(token, address, port) {
  if (!token) {
    logWarn("bootstrap 未返回令牌，二维码不可用");
    return;
  }
  const target = `http://${address}:${port}/#token=${token}`;
  $("qr-url").textContent = `http://${address}:${port}/`;
  $("qr").dataset.qrTarget = target;
  QRCode.toCanvas($("qr"), target, { width: 140, margin: 1 }, (err) => {
    if (err) logError("二维码生成失败", err);
  });
}


// 启动顺序（P4R Batch 3.5）：先确定连接地址，再确保持有会话令牌，最后连接。
// 令牌必须在**构造 socket 之前**就位，否则首次连接必然 401，页面会先显示一次
// 「已断开」再重连——那是引导时序缺陷被当成正常现象。
//
// 令牌不写入输入框，由 connect() 在建立连接时附加；每次重连都会重新读取
// `sessionToken`，因此「重连丢失令牌」在结构上不可能出现。
async function boot() {
  const derivedWs = sameOriginWsUrl();
  if (derivedWs) $("server-url").value = derivedWs;
  try {
    await ensureSessionToken();
  } catch (e) {
    logWarn("会话令牌引导异常，将以无令牌方式尝试连接", e);
  }
  connect();
}

boot();