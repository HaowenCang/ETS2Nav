// Browser → loopback 安全套件（P4R Batch 3.5 §19 BLS-01…BLS-06/BLS-08）。
//
// 本套件针对的是一个**实测复现过**的漏洞类别：服务端曾把 `PeerClass::Loopback`
// 当作身份，于是回环对端对所有动态 API 与 `/ws` 免令牌。攻击者不需要在受害者机器
// 上运行任何程序——只要让受害者的浏览器打开一个恶意页面，该页面就能向
// `http://127.0.0.1:<port>` 发出来源地址为回环的请求，服务端看到的对端同样是回环。
//
// 复现方式（真实 Chromium，非 HTTP 客户端模拟）：
//   · 跨源 simple POST（`Content-Type: text/plain` 属 CORS safelisted，无 preflight）
//     把导航目的地从 A 改成 B —— 经服务端真实 `map_state` 广播观测副作用；
//   · 跨站 WebSocket（WS 不走 CORS）直接读取车辆帧流。
//
// 判据设计的两条原则：
//   1. **只看副作用，不看攻击页面的返回值**。跨源请求在 `no-cors` 下恒返回 opaque
//      响应（`type:"opaque", status:0`），无论服务端接受还是拒绝都一样——用它的
//      status 作为判据等于什么都没断言。
//   2. **必须有已认证的正向对照**。只断言「没有出现目标 B」时，观测通道坏掉、
//      服务端路线功能坏掉都会让断言假通过；因此每个负向断言后面都跟一次带令牌的
//      真实目的地设置，并断言它确实产生广播。

import { expect, test } from "@playwright/test";
import { createServer } from "node:http";
import { connect } from "node:net";
import { bootstrap, freePort, session, startServer } from "../harness.mjs";
import { collectDiagnostics, assertCleanDiagnostics } from "./helpers.mjs";

// ─── 攻击页面 ────────────────────────────────────────────────────────────────

/**
 * 恶意页面：与 nav-server 不同端口（因此不同 origin），由本套件自己的 HTTP
 * 服务器提供。它只做浏览器允许它做的事，没有任何特权。
 */
const ATTACK_PAGE = `<!doctype html>
<meta charset="utf-8"><title>attacker</title>
<body><h1>attacker page</h1>
<script>
window.__csrfPlain = (navPort, from, to) => fetch("http://127.0.0.1:" + navPort + "/api/route", {
  method: "POST",
  mode: "no-cors",
  headers: { "Content-Type": "text/plain" },
  body: JSON.stringify({ from, to }),
}).then((r) => ({ ok: true, type: r.type, status: r.status }))
  .catch((e) => ({ ok: false, error: String(e) }));

window.__csrfJson = (navPort, from, to) => fetch("http://127.0.0.1:" + navPort + "/api/route", {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ from, to }),
}).then((r) => ({ ok: true, status: r.status }))
  .catch((e) => ({ ok: false, error: String(e) }));

window.__cswh = (navPort, token) => new Promise((res) => {
  const out = { opened: false, frames: 0, error: null, origin: location.origin };
  let sock;
  try {
    sock = new WebSocket("ws://127.0.0.1:" + navPort + "/ws" + (token ? "?token=" + token : ""));
  } catch (e) { out.error = String(e); res(out); return; }
  sock.onopen = () => { out.opened = true; };
  sock.onmessage = () => { out.frames++; };
  sock.onerror = () => { if (!out.error) out.error = "error"; };
  sock.onclose = () => setTimeout(() => res(out), 800);
  setTimeout(() => { try { sock.close(); } catch (e) {} }, 5000);
  setTimeout(() => res(out), 7000);
});

// 供 BLS-01 的「同源页面才能取到令牌」对照使用：尝试读取 bootstrap 响应体。
window.__readBootstrap = (navPort) => fetch("http://127.0.0.1:" + navPort + "/api/bootstrap")
  .then((r) => r.text()).then((t) => ({ ok: true, len: t.length, hasToken: /[0-9a-f]{64}/.test(t) }))
  .catch((e) => ({ ok: false, error: String(e) }));
</script>
</body>`;

// ─── 目标坐标（与协议/安全套件同一组已知可路由坐标）────────────────────────

const A = [-58456, 32832]; // 基线目的地（合法路径设定）
const B = [-52925, 36510]; // 攻击目标
const C = [-57000, 34500]; // 正向对照目的地

const near = (d, p) => Array.isArray(d) && d.length === 2
  && Math.abs(d[0] - p[0]) < 1 && Math.abs(d[1] - p[1]) < 1;

// ─── 观察与请求工具 ──────────────────────────────────────────────────────────

/**
 * 已认证的帧流观察通道（原生客户端：不发 Origin，令牌经 query 传递）。
 * 这是判断「服务端是否真的改变了导航目的地」的唯一观测口。
 */
function openObserver(port, token) {
  const seen = { frames: 0, vehicles: 0, mapStates: [] };
  const sock = new WebSocket(`ws://127.0.0.1:${port}/ws?token=${token}`);
  const ready = new Promise((ok) => {
    sock.onopen = () => ok(true);
    sock.onerror = () => ok(false);
    sock.onmessage = (m) => {
      try {
        const v = JSON.parse(m.data);
        seen.frames++;
        if (v.type === "vehicle") seen.vehicles++;
        if (v.type === "map_state") seen.mapStates.push(v.destination);
      } catch { /* 非 JSON 帧不计入 */ }
    };
    setTimeout(() => ok(sock.readyState === 1), 8000);
  });
  return { seen, ready, close: () => { try { sock.close(); } catch { /* 已关 */ } } };
}

/** 带令牌 POST /api/route，返回 HTTP 状态码。 */
async function authedRoute(port, token, from, to) {
  const r = await fetch(`http://127.0.0.1:${port}/api/route`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({ from, to }),
  });
  return r.status;
}

/** 等待某个目的地出现在 map_state 广播中（否则返回 false，由调用方断言）。 */
async function waitDest(seen, want, timeoutMs = 25_000) {
  const end = Date.now() + timeoutMs;
  while (Date.now() < end) {
    if (seen.mapStates.some((d) => near(d, want))) return true;
    await new Promise((r) => setTimeout(r, 200));
  }
  return false;
}

/**
 * 原始 WS 握手（不经浏览器）：用于断言握手的**字面状态码**。
 *
 * 浏览器只暴露「onerror」，无法区分 401 与 403；BLS-04 要求证明「持有正确令牌但
 * Origin 恶意仍被拒且是 403」，因此必须读取握手响应本身。
 * `origin === null` 表示不发送 Origin 头（原生客户端）。
 */
function rawHandshake(port, path, origin, hostHeader) {
  return new Promise((ok) => {
    const sock = connect(port, "127.0.0.1");
    let raw = "";
    const done = () => {
      try { sock.destroy(); } catch { /* 已关 */ }
      const line = raw.split("\r\n")[0] ?? "";
      const m = /^HTTP\/1\.1 (\d{3})/.exec(line);
      ok({ status: m ? Number(m[1]) : null, line, raw });
    };
    sock.setTimeout(8000, done);
    sock.on("error", () => { raw = raw || ""; done(); });
    sock.on("connect", () => {
      const key = Buffer.from(String(Math.random())).toString("base64");
      let req = `GET ${path} HTTP/1.1\r\nHost: ${hostHeader ?? `127.0.0.1:${port}`}\r\n`;
      if (origin !== null) req += `Origin: ${origin}\r\n`;
      req += `Upgrade: websocket\r\nConnection: Upgrade\r\n`
        + `Sec-WebSocket-Key: ${key}\r\nSec-WebSocket-Version: 13\r\n\r\n`;
      sock.write(req);
    });
    sock.on("data", (d) => {
      raw += d.toString("latin1");
      if (raw.includes("\r\n\r\n")) done();
    });
  });
}

// ─── 会话级固定装置 ──────────────────────────────────────────────────────────

let srv;          // nav-server，**默认回环模式**（不加 --lan）——攻击面最小的配置
let atk;          // 攻击页面服务器
let attackerPort;
let attackerOrigin;

test.beforeAll(async () => {
  // 钩子内自行启动 nav-server；预算需覆盖服务器就绪（冷缓存下可能显著长于热缓存）
  test.setTimeout(120_000);
  const s = await session();
  srv = await startServer({ webRoot: s.webRoot, trace: s.trace });
  attackerPort = await freePort();
  atk = createServer((req, res) => {
    res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
    res.end(ATTACK_PAGE);
  });
  await new Promise((ok) => atk.listen(attackerPort, "127.0.0.1", ok));
  attackerOrigin = `http://127.0.0.1:${attackerPort}`;
  console.log(`[bls] nav-server :${srv.port}（默认回环模式）  攻击页面 ${attackerOrigin}`);
});

test.afterAll(async () => {
  if (atk) await new Promise((ok) => atk.close(ok));
  if (srv) await srv.stop();
});

// ─── BLS-01 跨源 simple POST ────────────────────────────────────────────────

test("BLS-01 跨源 simple POST（text/plain）无法改写导航目的地", async ({ page }) => {
  const obs = openObserver(srv.port, srv.token);
  try {
    expect(await obs.ready, "已认证观察通道必须建立（否则负向断言无意义）").toBe(true);
    // 观测通道确实在收帧
    await expect.poll(() => obs.seen.vehicles, { timeout: 25_000 }).toBeGreaterThan(0);

    // 基线：合法路径设定目的地 A
    expect(await authedRoute(srv.port, srv.token, A, A)).toBe(200);
    expect(await waitDest(obs.seen, A), "带令牌的合法请求必须产生 map_state 广播").toBe(true);

    // 攻击：跨源 simple POST（无令牌；text/plain 不触发 preflight）
    const responses = [];
    page.on("response", (r) => {
      if (r.url().includes("/api/route")) responses.push(r.status());
    });
    await page.goto(`${attackerOrigin}/`, { waitUntil: "domcontentloaded" });
    const before = obs.seen.mapStates.length;
    const res = await page.evaluate(
      ([p, f, t]) => window.__csrfPlain(p, f, t), [srv.port, A, B]);

    // 攻击页面在 no-cors 下只能拿到 opaque 响应，其 status 恒为 0——不足以作为判据
    expect(res.ok, `攻击请求未发出: ${JSON.stringify(res)}`).toBe(true);
    expect(res.type, "跨源 no-cors 请求必然是 opaque").toBe("opaque");

    await page.waitForTimeout(6000);
    const after = obs.seen.mapStates.slice(before);
    expect(
      after.filter((d) => near(d, B)),
      `攻击目标 B 出现在了 map_state 广播中：${JSON.stringify(after)}`,
    ).toHaveLength(0);

    // 服务端确实收到了该请求并明确拒绝（浏览器收到的响应码，非攻击页面可见的 opaque）
    expect(
      responses,
      `未捕获到 /api/route 的响应码，实际=${JSON.stringify(responses)}`,
    ).toContain(401);

    // 正向对照：带令牌的请求仍然生效——证明「没有 B」不是观测通道坏掉
    expect(await authedRoute(srv.port, srv.token, A, C)).toBe(200);
    expect(await waitDest(obs.seen, C), "正向对照必须成功，否则负向断言无区分力").toBe(true);
  } finally {
    obs.close();
  }
});

// ─── BLS-02 跨源 JSON POST ─────────────────────────────────────────────────

test("BLS-02 跨源 JSON POST 不产生导航副作用", async ({ page }) => {
  const obs = openObserver(srv.port, srv.token);
  try {
    expect(await obs.ready).toBe(true);
    await expect.poll(() => obs.seen.vehicles, { timeout: 25_000 }).toBeGreaterThan(0);
    expect(await authedRoute(srv.port, srv.token, A, A)).toBe(200);
    expect(await waitDest(obs.seen, A)).toBe(true);

    const preflights = [];
    const posts = [];
    page.on("response", (r) => {
      if (!r.url().includes("/api/route")) return;
      if (r.request().method() === "OPTIONS") preflights.push(r.status());
      if (r.request().method() === "POST") posts.push(r.status());
    });

    await page.goto(`${attackerOrigin}/`, { waitUntil: "domcontentloaded" });
    const before = obs.seen.mapStates.length;
    const res = await page.evaluate(([p, f, t]) => window.__csrfJson(p, f, t), [srv.port, A, B]);
    await page.waitForTimeout(6000);

    // 无论被 preflight 拦下还是真实请求被拒，最终都不得有副作用
    const after = obs.seen.mapStates.slice(before);
    expect(
      after.filter((d) => near(d, B)),
      `跨源 JSON POST 产生了副作用：${JSON.stringify(after)}`,
    ).toHaveLength(0);
    // 攻击页面的 fetch 必须失败（浏览器因缺少 ACAO 而拒绝交出响应）
    expect(res.ok, `跨源 JSON POST 竟然成功: ${JSON.stringify(res)}`).toBe(false);
    // 若预检真的发出，则它必须被服务端拒绝
    for (const st of preflights) {
      expect(st, "非白名单 origin 的预检必须被拒").toBe(403);
    }
    for (const st of posts) {
      expect([401, 403, 415], "POST 不得成功").toContain(st);
    }

    // 正向对照
    expect(await authedRoute(srv.port, srv.token, A, C)).toBe(200);
    expect(await waitDest(obs.seen, C)).toBe(true);
  } finally {
    obs.close();
  }
});

// ─── BLS-03 跨站 WebSocket ─────────────────────────────────────────────────

test("BLS-03 跨站 WebSocket 不升级、读不到车辆帧流", async ({ page }) => {
  await page.goto(`${attackerOrigin}/`, { waitUntil: "domcontentloaded" });
  const res = await page.evaluate((p) => window.__cswh(p, null), srv.port);
  expect(res.opened, `跨站 WS 竟然打开了: ${JSON.stringify(res)}`).toBe(false);
  expect(res.frames, "跨站 WS 不得收到任何帧").toBe(0);

  // 服务端侧的字面证据：握手不得出现 101
  const hs = await rawHandshake(srv.port, "/ws", attackerOrigin);
  expect(hs.line, `握手响应: ${hs.raw.slice(0, 160)}`).not.toContain("101");
  expect(hs.status, `握手响应行: ${hs.line}`).toBe(403);
});

// ─── BLS-04 正确令牌 + 恶意 Origin ─────────────────────────────────────────

test("BLS-04 持有正确令牌但 Origin 恶意仍被拒（Origin 未被令牌覆盖）", async ({ page }) => {
  // 服务端字面证据
  const hs = await rawHandshake(srv.port, `/ws?token=${srv.token}`, attackerOrigin);
  expect(hs.status, `握手响应行: ${hs.line}`).toBe(403);
  expect(hs.line).not.toContain("101");

  // 浏览器侧：攻击页面即便拿到令牌也无法建立连接
  await page.goto(`${attackerOrigin}/`, { waitUntil: "domcontentloaded" });
  const res = await page.evaluate(([p, t]) => window.__cswh(p, t), [srv.port, srv.token]);
  expect(res.opened, `恶意 Origin 且带令牌的 WS 竟然打开: ${JSON.stringify(res)}`).toBe(false);
  expect(res.frames).toBe(0);

  // 反向对照：同一令牌、同一路径，仅把 Origin 换成同源 → 必须成功。
  // 没有这一步，「403」可能只是令牌或路径写错了。
  const okHs = await rawHandshake(srv.port, `/ws?token=${srv.token}`,
    `http://127.0.0.1:${srv.port}`);
  expect(okHs.status, `同源 Origin 应完成握手，实际: ${okHs.line}`).toBe(101);
});

// ─── BLS-05 原生客户端 ─────────────────────────────────────────────────────

test("BLS-05 无 Origin 的原生客户端凭令牌仍可连接并收帧", async () => {
  const hs = await rawHandshake(srv.port, `/ws?token=${srv.token}`, null);
  expect(hs.status, `原生客户端握手应成功，实际: ${hs.line}`).toBe(101);
  // 不接受 Authorization 头的原生客户端也可用 query 令牌；此处再验证 101 之后真的有帧
  const obs = openObserver(srv.port, srv.token);
  try {
    expect(await obs.ready).toBe(true);
    await expect.poll(() => obs.seen.vehicles, { timeout: 25_000 }).toBeGreaterThan(0);
  } finally {
    obs.close();
  }
  // 无 Origin 但无令牌：不得因为「看起来像原生客户端」而被放行
  const noTok = await rawHandshake(srv.port, "/ws", null);
  expect(noTok.status, `无令牌的原生客户端应 401，实际: ${noTok.line}`).toBe(401);
});

// ─── BLS-06 同源正式页面 ───────────────────────────────────────────────────

test("BLS-06 同源本机页面的 bootstrap/API/WS/route/重连全部可用", async ({ page }) => {
  const diag = collectDiagnostics(page);
  const wsUrls = [];
  await page.addInitScript(() => {
    window.__wsUrls = [];
    const Native = window.WebSocket;
    window.WebSocket = function (...args) {
      window.__wsUrls.push(String(args[0]));
      return new Native(...args);
    };
    window.WebSocket.prototype = Native.prototype;
  });

  // 无 fragment、无 sessionStorage：令牌只能来自 /api/bootstrap
  await page.goto(`http://127.0.0.1:${srv.port}/`, { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => map.loaded() === true, null, { timeout: 20_000 });
  await expect(page.locator("#conn-status")).toHaveClass(/on/, { timeout: 20_000 });

  expect(await page.evaluate(() => sessionToken), "页面必须经 bootstrap 取得令牌")
    .toBe(srv.token);
  expect(await page.evaluate(() => sessionStorage.getItem("ets2nav.sessionToken")))
    .toBe(srv.token);

  const urls = await page.evaluate(() => window.__wsUrls);
  wsUrls.push(...urls);
  expect(urls.length).toBeGreaterThanOrEqual(1);
  expect(urls[0], "WS 必须携带令牌").toContain(`?token=${srv.token}`);
  expect(urls[0]).toContain(`//127.0.0.1:${srv.port}/ws`);

  // 实时帧流到达 DOM
  await expect.poll(async () => Number(await page.locator("#speed-val").textContent()),
    { timeout: 25_000 }).toBeGreaterThan(0);

  // 真实 UI 设目的地 → 携带 Bearer 且 200
  const p = page.waitForResponse(
    (r) => r.url().includes("/api/route") && r.request().method() === "POST",
    { timeout: 25_000 });
  await page.fill("#dest-x", String(B[0]));
  await page.fill("#dest-z", String(B[1]));
  await page.click("#btn-route");
  const r = await p;
  expect(r.status()).toBe(200);
  expect(r.request().headers()["authorization"]).toBe(`Bearer ${srv.token}`);

  // 断线重连：主动关闭当前 socket，重连必须继续携带当前令牌
  const before = await page.evaluate(() => window.__wsUrls.length);
  await page.evaluate(() => {
    const sock = ws;
    sock.onopen = sock.onmessage = sock.onerror = null;
    sock.close();
  });
  await expect.poll(() => page.evaluate(() => window.__wsUrls.length), { timeout: 30_000 })
    .toBeGreaterThan(before);
  await expect(page.locator("#conn-status")).toHaveClass(/on/, { timeout: 30_000 });
  const all = await page.evaluate(() => window.__wsUrls);
  for (const u of all) {
    expect(u, "每一次连接（含重连）都必须携带令牌").toContain(`?token=${srv.token}`);
  }

  assertCleanDiagnostics(diag, "BLS-06");
});

// ─── BLS-08 回环受保护端点令牌矩阵 ────────────────────────────────────────

test("BLS-08 回环对端对全部受保护端点都必须出示令牌", async () => {
  const paths = ["/api/snapshot", "/api/metadata", "/api/search?q=a", "/api/settings"];
  for (const p of paths) {
    const anon = await fetch(`http://127.0.0.1:${srv.port}${p}`);
    expect(anon.status, `${p} 无令牌必须 401`).toBe(401);
    const authed = await fetch(`http://127.0.0.1:${srv.port}${p}`,
      { headers: { Authorization: `Bearer ${srv.token}` } });
    expect(authed.status, `${p} 带令牌必须 200`).toBe(200);
  }
  const anonPost = await fetch(`http://127.0.0.1:${srv.port}/api/route`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ from: A, to: A }),
  });
  expect(anonPost.status, "回环 POST /api/route 无令牌必须 401").toBe(401);
  const authedPost = await authedRoute(srv.port, srv.token, A, A);
  expect(authedPost, "回环 POST /api/route 带令牌必须 200").toBe(200);

  // query 令牌对 HTTP API 无效（只有 /ws 接受 query）
  const q = await fetch(`http://127.0.0.1:${srv.port}/api/snapshot?token=${srv.token}`);
  expect(q.status, "HTTP API 不得接受 query 令牌").toBe(401);

  // bootstrap 是唯一豁免，且只对回环开放
  const boot = await bootstrap("127.0.0.1", srv.port);
  expect(boot.token).toBe(srv.token);
  expect(boot.lan_enabled, "本次运行的服务器未加 --lan").toBe(false);
});

// ─── BLS-09 攻击页面无法读取 bootstrap ────────────────────────────────────

test("BLS-09 攻击页面读不到 bootstrap（回环对端 + 跨源 Origin 仍被拒）", async ({ page }) => {
  await page.goto(`${attackerOrigin}/`, { waitUntil: "domcontentloaded" });
  const res = await page.evaluate((p) => window.__readBootstrap(p), srv.port);
  expect(res.ok, `跨源读取 bootstrap 竟然成功: ${JSON.stringify(res)}`).toBe(false);
  // 服务端侧：同一请求必须是 403，且响应体不含令牌
  const r = await fetch(`http://127.0.0.1:${srv.port}/api/bootstrap`,
    { headers: { Origin: attackerOrigin } });
  expect(r.status).toBe(403);
  const body = await r.text();
  expect(body).not.toContain(srv.token);
});
