// ETS2Nav Browser E2E 测试环境（P4R Batch 2 §10）。
//
// 每个测试会话拥有自己的：临时 web root、合法 PMTiles fixture、动态端口、确定性
// 导航数据。绝不读取开发者源码目录中的 tools/ets2nav-web/map.pmtiles——那个文件
// 是本地未跟踪的历史非法产物，把它作为输入会让测试结果依赖开发机状态。
//
// 生命周期：globalSetup 建立一次（生成 fixture + trace、启动服务器），globalTeardown
// 清理。端口用「绑定 0 端口取实际值」的方式动态分配，避免固定端口冲突。

import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { existsSync } from "node:fs";
import { cp, mkdir, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const WEB_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const REPO_ROOT = resolve(WEB_DIR, "..", "..");
export const NAV_CLI = join(REPO_ROOT, "nav-core", "target", "debug", "nav-core-cli.exe");
export const DATASET = join(REPO_ROOT, "data", "europe-v5");
const FIXTURE_PROJ = join(
  REPO_ROOT, "map-compiler", "tools", "PmtilesFixture", "PmtilesFixture.csproj");

/**
 * 测试会话根目录（临时）。
 *
 * 目录名必须跨进程稳定：globalSetup 在 Playwright 运行器进程中执行，而 spec 在
 * 各 worker 子进程中执行，二者 PID 不同，故不能用 process.pid 构造路径。
 * 代价是同一时刻只能有一个 E2E 会话在跑（本项目 workers=1，且全局只跑一个套件）。
 */
export const SESSION_DIR = join(tmpdir(), "ets2nav-e2e");

/** 服务器启动超时（含 europe-v5 数据集加载；实测约 3 s，留足余量）。 */
const SERVER_BOOT_TIMEOUT_MS = 120_000;

/** 分配一个当前空闲的 TCP 端口（绑定 0 后立即释放；存在极小的竞态窗口）。 */
export function freePort() {
  return new Promise((ok, bad) => {
    const srv = createServer();
    srv.on("error", bad);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => ok(port));
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

async function waitForPort(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/api/snapshot`, {
        signal: AbortSignal.timeout(2000),
      });
      if (r.ok) return true;
    } catch { /* 尚未监听 */ }
    await new Promise((r) => setTimeout(r, 200));
  }
  return false;
}

/**
 * 构建隔离 web root：
 *   1. 复制 dist/ 的全部内容，但**排除任何 map.pmtiles**；
 *   2. 用正式 PmtilesWriter（经 PmtilesFixture 工具）现场生成合法归档；
 *   3. 断言归档确实存在且非空——fixture 生成失败必须让整个测试会话失败，
 *      而不是悄悄退化成「无底图模式」使地图断言变成空断言。
 */
export async function prepareWebRoot(log = console.log) {
  const dist = join(WEB_DIR, "dist");
  if (!existsSync(dist)) {
    throw new Error(`dist/ 不存在（${dist}）。请先执行 npm run build。`);
  }
  const webRoot = join(SESSION_DIR, "web");
  await rm(webRoot, { recursive: true, force: true });
  await mkdir(webRoot, { recursive: true });

  for (const name of await readdir(dist)) {
    if (name === "map.pmtiles") continue; // 显式排除：不得使用源码目录遗留档案
    await cp(join(dist, name), join(webRoot, name), { recursive: true });
  }

  log("[e2e] 生成 PMTiles fixture（正式 writer）…");
  const fixture = join(webRoot, "map.pmtiles");
  const r = await run("dotnet", [
    "run", "--project", FIXTURE_PROJ, "-c", "Debug", "--", fixture,
  ], { cwd: REPO_ROOT });
  if (r.code !== 0) {
    throw new Error(`PmtilesFixture 生成失败 (exit ${r.code}):\n${r.err}`);
  }
  const info = await import("node:fs/promises").then((m) => m.stat(fixture));
  if (!(info.size > 1024)) {
    throw new Error(`fixture 异常（${info.size} B）——地图断言将失去意义，测试中止`);
  }
  log(`[e2e] fixture: ${info.size} B`);
  return { webRoot, fixtureBytes: info.size };
}

/** 用正式 syntrace 生成确定性回放数据（柏林合成轨迹）。 */
export async function prepareTrace(log = console.log) {
  const out = join(SESSION_DIR, "e2e.navtrace");
  if (existsSync(out)) return out;
  log("[e2e] 生成合成 trace…");
  const r = await run(NAV_CLI, [
    "syntrace", "-58456,32832:-52925,36510", DATASET, out,
  ], { cwd: join(REPO_ROOT, "nav-core") });
  if (r.code !== 0 || !existsSync(out)) {
    throw new Error(`syntrace 失败 (exit ${r.code}):\n${r.err}`);
  }
  return out;
}

/**
 * 启动一个 nav-server 实例。
 * @param {{webRoot:string, trace?:string, fakeSignal?:boolean, port?:number, log?:Function}} opts
 *   port 省略时动态分配空闲端口；E2E-10 需要「同端口重启」故支持显式指定。
 */
export async function startServer({ webRoot, trace, fakeSignal = false, port, log = console.log }) {
  const chosen = port ?? await freePort();
  const args = [
    "server", DATASET,
    `--port=${chosen}`,
    `--web=${webRoot}`,
  ];
  if (trace) args.push(`--replay=${trace}`);
  if (fakeSignal) args.push("--fake-signal");
  const proc = spawn(NAV_CLI, args, {
    cwd: join(REPO_ROOT, "nav-core"),
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  let stderr = "";
  proc.stderr.on("data", (d) => { stderr += d; });
  proc.stdout.on("data", () => {});

  const up = await waitForPort(chosen, SERVER_BOOT_TIMEOUT_MS);
  if (!up) {
    proc.kill();
    throw new Error(`nav-server 未在 ${SERVER_BOOT_TIMEOUT_MS} ms 内监听 :${chosen}\n${stderr}`);
  }
  log(`[e2e] server :${chosen}${fakeSignal ? " (fake-signal)" : ""}`);
  return {
    pid: proc.pid,
    port: chosen,
    origin: `http://127.0.0.1:${chosen}`,
    stop: () => new Promise((ok) => {
      if (proc.exitCode !== null || proc.signalCode) return ok();
      proc.once("close", () => ok());
      proc.kill();
    }),
    get stderr() { return stderr; },
  };
}

export async function cleanupSession() {
  await rm(SESSION_DIR, { recursive: true, force: true });
}

export async function writeSessionInfo(info) {
  await mkdir(SESSION_DIR, { recursive: true });
  await writeFile(join(SESSION_DIR, "session.json"), JSON.stringify(info, null, 2), "utf8");
}

let cachedSession = null;

/**
 * 读取本会话的服务器信息。
 * 必须在 globalSetup 之后调用（配置加载阶段与测试收集阶段 early 于 setup，
 * 此时 session.json 尚不存在）。
 */
export async function session() {
  if (cachedSession) return cachedSession;
  const { readFile } = await import("node:fs/promises");
  cachedSession = JSON.parse(await readFile(join(SESSION_DIR, "session.json"), "utf8"));
  return cachedSession;
}
