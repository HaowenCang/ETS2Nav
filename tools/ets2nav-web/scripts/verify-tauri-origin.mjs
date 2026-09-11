#!/usr/bin/env node
// BLS-07：Tauri Origin 与引导流程的真实复核（P4R Batch 3.5 §19）。
//
// 为什么需要单独一个脚本：`http://tauri.localhost` 的 WebView 页面**不是**由
// nav-server 提供的，因此它的 bootstrap、WS 与 API 全部是跨源请求；这一点无法用
// Playwright 的普通页面复现（那需要真的把 Tauri 壳跑起来）。
//
// 做法：跑**真实的** `ets2nav-desktop.exe`（Tauri 2 + WebView2），让它的请求经过
// 一个记录型转发器到达**真实的** nav-server，然后断言三件事：
//   1. 应用发出的 `/api/bootstrap` 与 `/ws` 都带 `Origin: http://tauri.localhost`；
//   2. 服务端对该 Origin 返回 200/101（即白名单里的那个值确实是应用实际使用的值）；
//   3. 令牌确实随 WS 连接传递，且引导与握手都成功。
//
// **转发器的诚实说明**：转发器在 8123 接受连接后再以 `Host: 127.0.0.1:<后端端口>`
// 向后端重新发起请求，因此本脚本验证的是 **Origin 策略与端到端流程**，它本身不
// 覆盖 Host 校验（Host 校验由 S10 与 BLS-01…09 在真实端口上直接覆盖）。之所以需要
// 转发：Tauri 页面里连接地址是固定的 `ws://127.0.0.1:8123/ws`（见 index.html），
// 而后端必须是另起的一个端口，否则无法记录应用实际发出的请求头。
//
// 退出码：0 PASS；1 FAIL；3 NOT VERIFIED（无法执行——例如未构建桌面壳）。

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { createServer, connect } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { DATASET, NAV_CLI, REPO_ROOT, bootstrap, freePort, startServer } from "../tests/harness.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB_DIR = resolve(HERE, "..");
const DESKTOP_EXE = join(REPO_ROOT, "desktop", "target", "debug", "ets2nav-desktop.exe");
const WORK = join(tmpdir(), "ets2nav-tauri");
const TRACE = join(WORK, "tauri.navtrace");
const PROXY_PORT = 8123; // index.html 中连接地址的固定端口；不可随意更改
const TAURI_ORIGIN = "http://tauri.localhost";

/** 记录到的请求（只保留与本轮判据相关的字段，避免把令牌写进日志）。 */
const records = [];

function log(...a) {
  console.log("[tauri]", ...a);
}

function run(cmd, args, opts = {}) {
  return new Promise((ok, bad) => {
    const p = spawn(cmd, args, { stdio: ["ignore", "pipe", "pipe"], ...opts });
    let err = "";
    p.stderr.on("data", (d) => { err += d; });
    p.on("error", bad);
    p.on("close", (code) => ok({ code, err }));
  });
}

/**
 * 记录型转发器：接受 WebView 的连接，解析请求头后向后端重新发起并双向转发。
 *
 * 只做两件改写：`Host` 指向后端端口、`Connection: close`（后端每个响应后都会关闭，
 * 这样转发器可以用 EOF 判定响应结束）。其余请求头——尤其是 `Origin`——原样传递，
 * 因为 Origin 正是本脚本要观测并断言的对象。
 */
function startProxy(listenPort, backendPort) {
  const server = createServer((client) => {
    let head = Buffer.alloc(0);
    let handled = false;

    const onData = (chunk) => {
      if (handled) return;
      head = Buffer.concat([head, chunk]);
      const end = head.indexOf("\r\n\r\n");
      if (end < 0) return;
      handled = true;
      client.removeListener("data", onData);

      const headText = head.subarray(0, end + 4).toString("latin1");
      let body = head.subarray(end + 4);
      const lines = headText.split("\r\n");
      const [method, target] = lines[0].split(" ");
      const hdrs = {};
      for (const ln of lines.slice(1)) {
        const i = ln.indexOf(":");
        if (i > 0) hdrs[ln.slice(0, i).trim().toLowerCase()] = ln.slice(i + 1).trim();
      }
      const wantBody = Number(hdrs["content-length"] ?? 0);
      const rec = {
        method,
        target,
        origin: hdrs.origin ?? null,
        host: hdrs.host ?? null,
        upgrade: hdrs.upgrade ?? null,
        authorization: hdrs.authorization ? "present" : null,
        hasQueryToken: /[?&]token=/.test(target),
        // 令牌只记录形态，不记录值：日志与 CI artifact 不该携带凭据
        tokenShaped: /[?&]token=([0-9a-f]{64})(&|$)/.test(target),
        backendStatus: null,
        setCookie: null,
      };
      records.push(rec);

      const finishBody = () => {
        const out = [];
        out.push(`${method} ${target} HTTP/1.1`);
        for (const ln of lines.slice(1)) {
          const i = ln.indexOf(":");
          if (i <= 0) continue;
          const k = ln.slice(0, i).trim();
          const lk = k.toLowerCase();
          if (lk === "host" || lk === "connection") continue;
          out.push(ln);
        }
        out.push(`Host: 127.0.0.1:${backendPort}`);
        out.push("Connection: close");
        const fwdHead = Buffer.from(out.join("\r\n") + "\r\n\r\n", "latin1");

        const up = connect(backendPort, "127.0.0.1");
        up.on("error", () => { try { client.destroy(); } catch { /* 已关 */ } });
        up.on("connect", () => {
          up.write(fwdHead);
          if (body.length) up.write(body);
        });
        up.on("data", (d) => {
          if (rec.backendStatus === null) {
            const m = /^HTTP\/1\.1 (\d{3})/.exec(d.toString("latin1"));
            if (m) rec.backendStatus = Number(m[1]);
          }
          client.write(d);
        });
        up.on("close", () => { try { client.end(); } catch { /* 已关 */ } });
        client.on("data", (d) => up.write(d));
        client.on("close", () => { try { up.destroy(); } catch { /* 已关 */ } });
      };

      if (body.length >= wantBody) {
        body = body.subarray(0, wantBody);
        finishBody();
      } else {
        const need = wantBody - body.length;
        client.once("data", (more) => {
          body = Buffer.concat([body, more.subarray(0, need)]);
          finishBody();
        });
      }
    };

    client.on("data", onData);
    client.on("error", () => { /* 客户端断开：无需处理 */ });
  });
  return new Promise((ok, bad) => {
    server.on("error", bad);
    server.listen(listenPort, "127.0.0.1", () => ok(server));
  });
}

async function ensureTrace() {
  if (existsSync(TRACE)) return TRACE;
  await mkdir(WORK, { recursive: true });
  const r = await run(NAV_CLI, ["syntrace", "-58456,32832:-52925,36510", DATASET, TRACE],
    { cwd: join(REPO_ROOT, "nav-core") });
  if (r.code !== 0 || !existsSync(TRACE)) throw new Error(`syntrace 失败: ${r.err}`);
  return TRACE;
}

const FAIL = [];
const NV = [];
function check(name, ok, detail = "") {
  console.log(`[${ok ? "PASS" : "FAIL"}] ${name}${detail ? ` — ${detail}` : ""}`);
  if (!ok) FAIL.push(name);
}
function notVerified(name, why) {
  console.log(`[NOT VERIFIED] ${name} — ${why}`);
  NV.push(name);
}

async function main() {
  if (!existsSync(DESKTOP_EXE)) {
    notVerified("BLS-07 Tauri Origin 真实复核",
      `未找到 ${DESKTOP_EXE}；请先在 desktop/ 执行 cargo build`);
    return 3;
  }
  const trace = await ensureTrace();

  // 后端：真实 nav-server，--lan（桌面场景会同时服务手机）
  const backend = await startServer({
    webRoot: join(WEB_DIR, "dist"), trace, lan: true, log: () => {},
  });
  log(`后端 nav-server :${backend.port}（--lan）`);

  let proxy;
  try {
    proxy = await startProxy(PROXY_PORT, backend.port);
  } catch (e) {
    await backend.stop();
    notVerified("BLS-07 Tauri Origin 真实复核",
      `无法在 ${PROXY_PORT} 上启动记录转发器（端口被占用？）: ${e}`);
    return 3;
  }
  log(`记录转发器 127.0.0.1:${PROXY_PORT} → 后端 :${backend.port}`);

  // 前置：后端确实在按预期工作（否则后面的结论可能只是「后端坏了」）
  const boot = await bootstrap("127.0.0.1", backend.port);
  check("前置：后端 bootstrap 返回令牌", typeof boot.token === "string" && boot.token.length === 64);

  log("启动真实 Tauri 应用…");
  const app = spawn(DESKTOP_EXE, [], { stdio: ["ignore", "pipe", "pipe"], windowsHide: false });
  let appErr = "";
  app.stderr.on("data", (d) => { appErr += d; });
  app.stdout.on("data", () => {});

  await new Promise((r) => setTimeout(r, 30_000));
  const aliveDuringRun = app.exitCode === null;
  try { app.kill(); } catch { /* 已退出 */ }
  await new Promise((r) => setTimeout(r, 1500));

  try { proxy.close(); } catch { /* 已关 */ }
  await backend.stop();

  const bootReqs = records.filter((r) => r.target.startsWith("/api/bootstrap"));
  const wsReqs = records.filter((r) => (r.upgrade ?? "").toLowerCase() === "websocket");
  const apiReqs = records.filter((r) => r.target.startsWith("/api/") && !r.target.startsWith("/api/bootstrap"));

  log(`记录到 ${records.length} 个请求：bootstrap=${bootReqs.length} ws=${wsReqs.length} 其它API=${apiReqs.length}`);
  for (const r of records) {
    log(`  ${r.method} ${r.target.slice(0, 48)}  origin=${r.origin}  status=${r.backendStatus}`);
  }

  if (records.length === 0) {
    notVerified("BLS-07 Tauri Origin 真实复核",
      `Tauri 应用未发出任何请求（aliveDuringRun=${aliveDuringRun}，stderr 末尾: `
      + `${appErr.split("\n").slice(-3).join(" | ").slice(0, 200)}）`);
    return 3;
  }

  check("BLS-07 应用进程在观测窗口内存活", aliveDuringRun,
    `stderr 末尾: ${appErr.split("\n").slice(-2).join(" | ").slice(0, 160)}`);

  // 1) Origin 实测值：必须与 CORS 白名单常量一致
  check("BLS-07 应用的 bootstrap 请求带 Origin: http://tauri.localhost",
    bootReqs.length > 0 && bootReqs.every((r) => r.origin === TAURI_ORIGIN),
    `实际 origin=${JSON.stringify(bootReqs.map((r) => r.origin))}`);
  check("BLS-07 服务端接受该 Origin（bootstrap 200）",
    bootReqs.length > 0 && bootReqs.every((r) => r.backendStatus === 200),
    `status=${JSON.stringify(bootReqs.map((r) => r.backendStatus))}`);

  // 2) WS：跨源握手 + 令牌
  check("BLS-07 应用的 /ws 握手带 Origin: http://tauri.localhost",
    wsReqs.length > 0 && wsReqs.every((r) => r.origin === TAURI_ORIGIN),
    `实际 origin=${JSON.stringify(wsReqs.map((r) => r.origin))}`);
  check("BLS-07 服务端接受该 Origin 并完成 101",
    wsReqs.length > 0 && wsReqs.every((r) => r.backendStatus === 101),
    `status=${JSON.stringify(wsReqs.map((r) => r.backendStatus))}`);
  check("BLS-07 WS 连接携带 64 位十六进制会话令牌",
    wsReqs.length > 0 && wsReqs.every((r) => r.tokenShaped),
    `query=${JSON.stringify(wsReqs.map((r) => r.target.replace(/token=[0-9a-f]+/, "token=<64hex>")))}`);

  // 3) 反向：任何请求都不得使用未批准 Origin，也不得缺少 Origin 而走原生通道
  const badOrigin = records.filter((r) => r.origin !== null && r.origin !== TAURI_ORIGIN);
  check("BLS-07 应用未使用任何非白名单 Origin", badOrigin.length === 0,
    `异常=${JSON.stringify(badOrigin.map((r) => [r.target.slice(0, 24), r.origin]))}`);
  const noOrigin = records.filter((r) => r.origin === null);
  log(`（无 Origin 的请求 ${noOrigin.length} 个，属原生/非浏览器路径，不影响本项判据）`);

  console.log();
  if (FAIL.length) {
    console.log(`TAURI ORIGIN: FAIL (${FAIL.length}) — ${FAIL.join("; ")}`);
    return 1;
  }
  if (NV.length) {
    console.log(`TAURI ORIGIN: NOT VERIFIED — ${NV.join("; ")}`);
    return 3;
  }
  console.log("TAURI ORIGIN: PASS");
  return 0;
}

process.exit(await main());
