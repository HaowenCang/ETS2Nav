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
import { networkInterfaces, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const WEB_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const REPO_ROOT = resolve(WEB_DIR, "..", "..");
export const NAV_CLI = join(REPO_ROOT, "nav-core", "target", "debug", "nav-core-cli.exe");

/**
 * 导航数据集根目录。
 *
 * 优先级与仓库其余部分保持同一套契约（P4R Batch 4 建立，Batch 5 §14 接入本文件）：
 *   环境变量 ETS2NAV_DATASET  >  repo 相对默认值 data/europe-v5
 *
 * 之所以必须读环境变量：CI 在 checkout 之外准备数据集（166 MB 归档解压 356 MB，
 * 写进工作区既慢又会污染"干净检出"的判定），再把绝对路径经 ETS2NAV_DATASET 传入。
 * 若此处另立一套优先级（例如再加一个 ETS2NAV_DATA_DIR），同一次运行就可能出现
 * "PowerShell harness 用一个数据集、浏览器 E2E 用另一个"的静默分歧。
 */
export const DATASET = process.env.ETS2NAV_DATASET && process.env.ETS2NAV_DATASET.trim()
  ? resolve(process.env.ETS2NAV_DATASET.trim())
  : join(REPO_ROOT, "data", "europe-v5");

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

/**
 * 服务器启动超时（含 europe-v5 数据集加载与 search.db 读取）。
 *
 * 实测：磁盘缓存热时约 3 s；但在**冷缓存 + 并发 I/O**（例如同一会话刚跑完
 * `dotnet run` 生成 PMTiles fixture）下曾观察到远超 45 s 的启动时间——服务端在
 * `TcpListener::bind` 之后、accept 循环之前还要读取 search.db，因此「已绑定端口」
 * 不等于「已可服务」。取 120 s 以避免把 I/O 抖动当成产品缺陷，同时保持失败信息
 * 可诊断（进程提前退出即立刻失败，并携带退出码与 stderr）。
 */
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

/**
 * 向某地址请求 `/api/bootstrap`，返回解析后的 JSON（非 200 时返回
 * `{status, body}`）。
 *
 * P4R Batch 3.5：这是**唯一的令牌出口**，且只对回环对端开放。测试经由它与
 * 正式客户端走同一条路径取得令牌——不存在 `--disable-auth` 之类的测试后门：
 * 若该端点被破坏，全部需要令牌的断言会一并失败。
 *
 * `host` 必须是服务器认得的 authority（回环名或本机实际地址）；否则服务端会因
 * Host 校验失败而 403——那正是 Host 策略生效的证据，而不是测试环境问题。
 */
export async function bootstrap(host, port) {
  const r = await fetch(`http://${host}:${port}/api/bootstrap`, {
    signal: AbortSignal.timeout(5000),
  });
  const text = await r.text();
  if (!r.ok) return { status: r.status, body: text };
  return JSON.parse(text);
}

/**
 * 等待服务器就绪。
 *
 * 判据是 bootstrap 返回 200 且带合法令牌，而不再是「/api/snapshot 返回 ok」——
 * Batch 3.5 之后未认证的 /api/snapshot 必然 401，用它当就绪信号会把「服务器已
 * 就绪」误判为「服务器未启动」。
 *
 * 两种提前失败条件都必须显式处理，否则故障会退化为「等满超时」这种无信息的失败：
 *   · 子进程已退出（bind 失败、数据集缺失）→ 立即返回，不再空等；
 *   · 最近一次探测的**实际结果**随返回值给出（连接被拒 / 状态码非 200 / 响应体
 *     畸形三者含义完全不同，不该被同一个「未监听」掩盖）。
 */
async function waitForPort(port, timeoutMs, exited = () => null) {
  const deadline = Date.now() + timeoutMs;
  const started = Date.now();
  let last = "尚未发起探测";
  while (Date.now() < deadline) {
    const code = exited();
    if (code !== null) return { ok: false, last, exitCode: code, elapsedMs: Date.now() - started };
    try {
      const r = await fetch(`http://127.0.0.1:${port}/api/bootstrap`, {
        signal: AbortSignal.timeout(3000),
      });
      const text = await r.text();
      last = `status=${r.status} body=${text.slice(0, 120)}`;
      if (r.ok) {
        const d = JSON.parse(text);
        if (typeof d.token === "string" && d.token.length === 64) {
          return { ok: true, last, elapsedMs: Date.now() - started };
        }
        last += " （令牌缺失或形态非法）";
      }
    } catch (e) {
      last = `异常 ${e?.name ?? "Error"}: ${String(e).slice(0, 120)}`;
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  return { ok: false, last, exitCode: exited(), elapsedMs: Date.now() - started };
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
 * @param {{webRoot:string, trace?:string, fakeSignal?:boolean, port?:number, lan?:boolean, log?:Function}} opts
 *   port 省略时动态分配空闲端口；E2E-10 需要「同端口重启」故支持显式指定。
 *   lan=true 加 `--lan`（绑定 0.0.0.0）。
 *
 * 返回的 `token` 由 `/api/bootstrap` 取得——**不是**测试注入的旁路凭据。
 * 两种模式下动态 API 与 /ws 都要求令牌（Batch 3.5），因此测试必须像真实客户端
 * 一样先引导再访问。
 */
export async function startServer({
  webRoot, trace, fakeSignal = false, port, lan = false, log = console.log,
}) {
  const chosen = port ?? await freePort();
  const args = [
    "server", DATASET,
    `--port=${chosen}`,
    `--web=${webRoot}`,
  ];
  if (trace) args.push(`--replay=${trace}`);
  if (fakeSignal) args.push("--fake-signal");
  if (lan) args.push("--lan");
  const proc = spawn(NAV_CLI, args, {
    cwd: join(REPO_ROOT, "nav-core"),
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
  });
  let stderr = "";
  proc.stderr.on("data", (d) => { stderr += d; });
  proc.stdout.on("data", () => {});

  const up = await waitForPort(chosen, SERVER_BOOT_TIMEOUT_MS, () => proc.exitCode);
  if (!up.ok) {
    proc.kill();
    throw new Error(
      `nav-server 未在 ${SERVER_BOOT_TIMEOUT_MS} ms 内于 :${chosen} 就绪`
      + `（等待 ${up.elapsedMs} ms；exitCode=${up.exitCode ?? "仍在运行"}）\n`
      + `最后一次探测: ${up.last}\n`
      + `args: ${args.join(" ")}\n${stderr}`,
    );
  }
  const boot = await bootstrap("127.0.0.1", chosen);
  if (!boot || typeof boot.token !== "string") {
    throw new Error(`bootstrap 未返回令牌（端口 :${chosen}）: ${JSON.stringify(boot)}`);
  }
  log(`[e2e] server :${chosen}${fakeSignal ? " (fake-signal)" : ""}${lan ? " (lan)" : ""}`
    + ` 就绪耗时 ${up.elapsedMs} ms`);
  return {
    pid: proc.pid,
    port: chosen,
    lan,
    token: boot.token,
    bootMs: up.elapsedMs,
    origin: `http://127.0.0.1:${chosen}`,
    stop: () => new Promise((ok) => {
      if (proc.exitCode !== null || proc.signalCode) return ok();
      proc.once("close", () => ok());
      proc.kill();
    }),
    get stderr() { return stderr; },
  };
}

/**
 * 枚举本机 RFC1918 候选地址（经服务器 bootstrap）。
 *
 * 测试机可能没有任何私网地址（例如仅公网 IP 的构建机），此时二维码分支无法
 * 成立；调用方据此显式选择断言分支，而不是假装通过。
 */
export async function lanCandidates(host, port) {
  const d = await bootstrap(host, port);
  return d && d.lan_enabled === true && Array.isArray(d.addresses) ? d.addresses : [];
}

/** 该 IPv4 是否属 RFC1918（与 rust 侧 `security::classify_peer` 同一口径）。 */
export function isRfc1918(ip) {
  const [a, b] = ip.split(".").map(Number);
  return a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168);
}

/**
 * 本机**非** RFC1918 的非回环 IPv4 地址（服务端会判为 `Disallowed`）。
 *
 * 存在的理由：要真实执行「不受允许的来源被拒绝」这条断言，必须让服务端看到一个
 * 既非回环、也非私网的对端地址。本机的 VPN/隧道地址（如 198.18.0.0/15）恰好满足
 * ——连到它时内核选用的源地址就是它本身，于是服务端看到的是真实的不受允许来源，
 * 而不是伪造的。可用的具体地址依机器而定，故此处枚举后由调用方逐个探测连通性。
 *
 * 链路本地（169.254/16）也在 Disallowed 之列，但通常不可连接，一并列入由探测筛选。
 */
export function disallowedLocalAddresses() {
  const out = [];
  for (const list of Object.values(networkInterfaces())) {
    for (const ni of list ?? []) {
      if (ni.family !== "IPv4" || ni.internal) continue;
      if (ni.address === "127.0.0.1" || isRfc1918(ni.address)) continue;
      out.push(ni.address);
    }
  }
  return out;
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
