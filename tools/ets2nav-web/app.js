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
  const sc = $("signal-card");
  if (snap.upcoming_signal && snap.upcoming_signal.state) {
    sc.classList.remove("hidden");
    const st = snap.upcoming_signal.state.replace(/^LightState::/, "");
    $("signal-state").textContent = st;
    $("signal-state").className = st.toLowerCase();
    $("signal-remaining").textContent = snap.upcoming_signal.remaining_s != null
      ? "剩余 " + snap.upcoming_signal.remaining_s.toFixed(1) + "s" : "";
  } else {
    sc.classList.add("hidden");
    $("glosa").textContent = "";
  }

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
let ws = null;
function connect() {
  const url = $("server-url").value.trim();
  $("conn-status").textContent = "连接中…";
  $("conn-status").className = "";
  ws = new WebSocket(url);
  ws.onopen = () => {
    $("conn-status").textContent = "已连接 " + url;
    $("conn-status").className = "on";
    // §61：展示同网移动端可用地址（server 监听 0.0.0.0——用当前 host 换协议）
    try {
      const httpBase = url.replace(/^ws:\/\//, "http://").replace(/\/ws$/, "");
      const displayUrl = "http://" + location.hostname + ":" + new URL(httpBase).port + "/";
      $("qr-url").textContent = displayUrl;
      // qrcode@1.5.4（官方维护，MIT）：API 为 toCanvas(canvas, text, options, cb)，
      // 取代此前未版本化 vendor 的 new QRCode(el, {...})。
      QRCode.toCanvas($("qr"), displayUrl, { width: 140, margin: 1 }, (err) => {
        if (err) { logError("二维码生成失败", err); return; }
        $("qr-box").classList.remove("hidden");
      });
    } catch (e) {
      logError("二维码地址构造失败", e);
    }
  };
  ws.onmessage = (ev) => {
    try { dispatchMessage(ev.data); } catch (e) { console.error("[ets2nav] 帧处理异常:", e); }
  };
  ws.onclose = () => { $("conn-status").textContent = "已断开"; $("conn-status").className = ""; };
  ws.onerror = () => { $("conn-status").textContent = "连接错误"; $("conn-status").className = ""; };
}

// 路由
async function setRoute() {
  const fx = parseFloat($("dest-x").value), fz = parseFloat($("dest-z").value);
  if (!isFinite(fx) || !isFinite(fz)) { alert("请输入终点 x/z 坐标"); return; }
  // 起点 = 车辆当前位置（无则用默认 Berlin 点）
  const [px, pz] = lastPos || [-58456, 32832];
  const base = $("server-url").value.replace(/^ws:\/\//, "http://").replace(/\/ws$/, "");
  const r = await fetch(base + "/api/route", {
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

// 自动连接
connect();
