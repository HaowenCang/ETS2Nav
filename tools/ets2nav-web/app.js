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
    glyphs: "https://demotiles.maplibre.org/font/{fontstack}/{range}.pbf",
  },
  center: toLngLat([-58400, 33000]),
  zoom: 10,
});

// 瓦片层（map.pmtiles 存在时；A2c-M5 修复：addSource 必须在 style load 后——
// 原实现 IIFE 内 await HEAD 后立即 addSource，style 未加载时抛错被 catch 吞掉，瓦片层静默缺失）
async function loadTiles() {
  try {
    const r = await fetch("map.pmtiles", { method: "HEAD" });
    if (!r.ok) return;
    map.addSource("tiles", { type: "pmtiles", url: "map.pmtiles" });
    map.addLayer({ id: "tile-road", type: "line", source: "tiles", "source-layer": "road",
      paint: { "line-color": "#3b82f6", "line-width": 1.2, "line-opacity": 0.75 } });
    map.addLayer({ id: "tile-junction", type: "circle", source: "tiles", "source-layer": "junction",
      paint: { "circle-color": "#f59e0b", "circle-radius": 2 } });
    map.addLayer({ id: "tile-city", type: "symbol", source: "tiles", "source-layer": "city",
      layout: { "text-field": ["get", "name"], "text-size": 11, "text-font": ["Open Sans Regular"] },
      paint: { "text-color": "#fff" } });
    map.addLayer({ id: "tile-poi", type: "circle", source: "tiles", "source-layer": "poi",
      paint: { "circle-color": "#10b981", "circle-radius": 2.5 } });
  } catch (e) { /* 无瓦片时纯路线渲染 */ }
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
// v km/h → 速度越快视野越大；D 越小越近；复杂路口放大
function autoZoom(snap) {
  const v = Math.max(snap.speed_kmh || 0, 10);
  const d = snap.next_maneuver ? Math.max(snap.next_maneuver.distance_m, 30) : 500;
  const complex = snap.next_maneuver && snap.next_maneuver.roundabout_exit != null ? 1.2 : 1.0;
  const z = 13 + Math.log2(Math.max(10, d)) * -0.55 + Math.log2(v / 40) * 0.5;
  return Math.max(9, Math.min(16.5, z * complex));
}

let following = true;
let lastPos = null;
let lastReminderKey = "";

function onSnapshot(snap) {
  // 状态
  const chip = $("state-chip");
  chip.textContent = snap.state.toUpperCase();
  chip.className = snap.state === "navigating" ? "navigating" : snap.state === "rerouting" ? "rerouting" : "";

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
      const qr = $("qr");
      qr.innerHTML = "";
      new QRCode(qr, { text: displayUrl, width: 140, height: 140 });
      $("qr-box").classList.remove("hidden");
    } catch (e) { /* 非浏览器环境跳过 */ }
  };
  ws.onmessage = (ev) => {
    try { onSnapshot(JSON.parse(ev.data)); } catch (e) { /* 忽略坏帧 */ }
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
$("btn-follow").onclick = () => { following = true; $("follow-hint").classList.add("hidden"); };
map.on("dragstart", () => { if (following) { following = false; $("follow-hint").classList.remove("hidden"); } });
map.on("wheel", () => { if (following) { following = false; $("follow-hint").classList.remove("hidden"); } });

// 自动连接
connect();
