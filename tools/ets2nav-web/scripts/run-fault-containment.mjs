// nav-server 数据源故障遏制验证（P4R Batch 6A §3/§4）。
//
// 被判定的性质（FC-01…FC-06）：
//   FC-01 数据源 worker 的 panic 被 supervisor 观察（stderr 出现注入点原文与致命记录）
//   FC-02 服务端不保持假健康：降级窗口内 listener 仍可连，但一律 503；且
//        「最后一帧 → 进程退出」的 zombie 窗口有上界
//   FC-03 进程退出码非零，且恰为契约值 70（与「启动失败 1」「用法错误 2」可区分）
//   FC-04 stderr 给出明确的致命类别（SOURCE_WORKER_PANIC / SOURCE_WORKER_EXIT）
//   FC-05 stderr 不含会话令牌
//   FC-06 已连接的 WS 客户端会失去连接（收到关闭），而不是永久停在最后一帧
//
// 关键设计：**主动制造故障**。等待「以后如果又碰巧出现一个 panic」不是验证。
// 注入开关只在 debug 构建中存在（`fault::inject` 由 cfg(debug_assertions) 门控），
// 因此本脚本首先断言被测 debug 二进制**确实**编译进了注入钩子——否则「注入无效」
// 会退化成「服务器正常运行到超时」这种无信息的失败。
//
// 本脚本绝不打印会话令牌；任何 URL 在进入日志前先经 redact()。

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, rmSync, statSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { DATASET, NAV_CLI } from "../tests/harness.mjs";

const INJECT_ENV = "ETS2NAV_FAULT_INJECT";
const HOLD_ENV = "ETS2NAV_FAULT_HOLD_MS";

/** zombie 窗口上界（毫秒）：最后一帧 → 进程退出。 */
const ZOMBIE_BOUND_MS = 3000;
/** 降级窗口观测时长（毫秒）；必须显著大于「判定顺序」的耗时。 */
const HOLD_MS = 1500;
/** 单次场景的整体超时。 */
const SCENARIO_TIMEOUT_MS = 60_000;
/**
 * 「worker 致命 → 进程退出」的有界等待。实测优秀实现约 30 ms；取 8 s 是为了把
 * 「机制完全缺失」（永不退出）与「调度抖动」区分开，而不是给慢实现留余量。
 */
const EXIT_TIMEOUT_MS = 8_000;
/** panic 场景重复次数（上界不取单点样本）。 */
const PANIC_REPEATS = 5;

const WORK_DIR = join(tmpdir(), "ets2nav-fc");

const results = [];
function check(name, ok, detail) {
  results.push({ name, ok, detail });
  console.log(`  [${ok ? "PASS" : "FAIL"}] ${name}${detail ? ` — ${detail}` : ""}`);
}

function redact(text) {
  return String(text).replace(/token=[A-Za-z0-9]+/g, "token=<redacted>");
}

function freePort() {
  return new Promise((ok, bad) => {
    const srv = createServer();
    srv.on("error", bad);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => ok(port));
    });
  });
}

function percentile(sorted, p) {
  if (sorted.length === 0) return null;
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[Math.max(0, idx)];
}

function stats(values) {
  const s = [...values].sort((a, b) => a - b);
  return {
    min: s[0], median: percentile(s, 50), p95: percentile(s, 95), max: s[s.length - 1], n: s.length,
  };
}

/** 生成确定性回放数据（本脚本自带，不复用 E2E 会话目录）。 */
async function prepareTrace() {
  const out = join(WORK_DIR, "fc.navtrace");
  if (existsSync(out)) return out;
  const r = await run(NAV_CLI, ["syntrace", "-58456,32832:-52925,36510", DATASET, out], {
    cwd: join(WORK_DIR),
  });
  if (r.code !== 0 || !existsSync(out)) {
    throw new Error(`syntrace 失败 (exit ${r.code}): ${r.err}`);
  }
  return out;
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
 * 启动 nav-server 并返回可观测句柄。
 *
 * `stderrLines` 逐行累积并带到达时间戳——FC-02 的「降级窗口何时开始」与 FC-03 的
 * 「进程何时退出」必须可分别定位，否则「503 是在降级前还是降级后观测到的」无法判定。
 */
async function startFaultyServer({ trace, webRoot, inject, holdMs }) {
  const port = await freePort();
  const args = [
    "server", DATASET, `--port=${port}`, `--web=${webRoot}`, `--replay=${trace}`, "--fake-signal",
  ];
  const env = { ...process.env, [INJECT_ENV]: inject };
  if (holdMs) env[HOLD_ENV] = String(holdMs);
  else delete env[HOLD_ENV];
  const proc = spawn(NAV_CLI, args, {
    cwd: join(WORK_DIR),
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
    env,
  });
  const stderrChunks = [];
  const events = [];
  let buf = "";
  proc.stderr.on("data", (d) => {
    stderrChunks.push(d.toString());
    buf += d.toString();
    let nl;
    while ((nl = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, nl);
      buf = buf.slice(nl + 1);
      events.push({ at: Date.now(), line });
    }
  });
  proc.stdout.on("data", () => {});
  const exited = new Promise((ok) => {
    proc.once("close", (code, signal) => ok({ code, signal, at: Date.now() }));
  });
  return {
    port,
    proc,
    events,
    exited,
    stderr: () => stderrChunks.join(""),
    /** 首条包含 `needle` 的 stderr 行的到达时间。 */
    firstLineAt(needle) {
      const hit = events.find((e) => e.line.includes(needle));
      return hit ? hit.at : null;
    },
    stop() {
      if (proc.exitCode === null && !proc.signalCode) proc.kill();
    },
  };
}

/** 等待服务器就绪（bootstrap 200 + 合法令牌），并返回令牌。 */
async function waitReady(port, srv, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let last = "未探测";
  while (Date.now() < deadline) {
    if (srv.proc.exitCode !== null) return { ok: false, last, exitCode: srv.proc.exitCode };
    try {
      const r = await fetch(`http://127.0.0.1:${port}/api/bootstrap`, {
        signal: AbortSignal.timeout(2000),
      });
      const text = await r.text();
      last = `status=${r.status}`;
      if (r.ok) {
        const d = JSON.parse(text);
        if (typeof d.token === "string" && d.token.length === 64) return { ok: true, token: d.token };
        last += " 令牌缺失或形态非法";
      }
    } catch (e) {
      last = `异常 ${e?.name ?? "Error"}`;
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  return { ok: false, last, exitCode: null };
}

/** 打开一个真实 WS 客户端并记录帧/关闭时刻。 */
function openWs(port, token) {
  const ws = new WebSocket(`ws://127.0.0.1:${port}/ws?token=${token}`);
  const obs = { frames: 0, firstFrameAt: null, lastFrameAt: null, closedAt: null, closeCode: null, errorAt: null, closeSeen: false };
  ws.addEventListener("message", () => {
    obs.frames += 1;
    const now = Date.now();
    if (obs.firstFrameAt === null) obs.firstFrameAt = now;
    obs.lastFrameAt = now;
  });
  ws.addEventListener("close", (e) => {
    obs.closedAt = Date.now();
    obs.closeCode = e.code;
    obs.closeSeen = true;
  });
  ws.addEventListener("error", () => { if (obs.errorAt === null) obs.errorAt = Date.now(); });
  obs.ws = ws;
  return obs;
}

/** 等待但**不抛**：把「没等到」作为一条判据交给调用方，而不是让整套件以异常结束。 */
async function waitForSoft(pred, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (pred()) return true;
    await new Promise((r) => setTimeout(r, 20));
  }
  return false;
}

async function waitFor(pred, timeoutMs, what) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (pred()) return true;
    await new Promise((r) => setTimeout(r, 20));
  }
  throw new Error(`等待超时（${timeoutMs} ms）：${what}`);
}

/**
 * 有界等待进程退出。
 *
 * 为什么必须有界：本套件的判据之一是「worker 致命 → 进程非零退出」。如果被测实现
 * 不退出（正是 zombie 形态），无界等待会让整个套件**挂住**——挂住不是失败，它既没有
 * 诊断也没有退出码，观测方无法区分「被测行为错误」与「测试自身卡了」。因此超时即记为
 * 失败，并显式杀掉进程避免留下一个仍在监听的孤儿。
 *
 * 返回 { code, signal, at, timedOut }。
 */
async function waitExit(srv, timeoutMs = EXIT_TIMEOUT_MS) {
  const timedOut = new Promise((ok) => setTimeout(() => ok({ code: null, signal: null, at: null, timedOut: true }), timeoutMs));
  const r = await Promise.race([srv.exited.then((e) => ({ ...e, timedOut: false })), timedOut]);
  if (r.timedOut) {
    srv.stop();
    // 给进程一点时间真正消失，避免影响下一个场景的端口分配
    await new Promise((ok) => setTimeout(ok, 500));
  }
  return r;
}

/** 带令牌的 GET，返回状态码（不读 body 内容之外的任何敏感信息）。 */
async function authedGet(port, path, token) {
  try {
    const r = await fetch(`http://127.0.0.1:${port}${path}`, {
      headers: { Authorization: `Bearer ${token}` },
      signal: AbortSignal.timeout(3000),
    });
    return { status: r.status, body: (await r.text()).slice(0, 200) };
  } catch (e) {
    return { status: null, error: String(e?.name ?? e) };
  }
}

// ─── 场景 ────────────────────────────────────────────────────────────────────

/**
 * 场景 A/B：worker 终止（panic 或意外正常返回）→ supervisor 观察 → 进程退出。
 */
async function scenario({ trace, webRoot, inject, expectClass, expectDetail, repeat }) {
  const srv = await startFaultyServer({ trace, webRoot, inject });
  const ready = await waitReady(srv.port, srv);
  if (!ready.ok) {
    srv.stop();
    check(`${inject} 场景就绪`, false, `${ready.last} exitCode=${ready.exitCode}`);
    return null;
  }
  const token = ready.token;
  const obs = openWs(srv.port, token);
  // 客户端必须确实收到过帧：否则 FC-06 的「停在最后一帧」没有指称对象。
  let framesOk = true;
  try {
    await waitFor(() => obs.frames >= 1, 20_000, `${inject}: 首帧`);
  } catch (e) {
    framesOk = false;
    check(`FC-06 ${inject}：客户端在故障前收到过帧`, false, String(e.message));
  }
  const exit = await waitExit(srv);
  let closeOk = true;
  try {
    await waitFor(() => obs.closeSeen, 5_000, `${inject}: WS 关闭事件`);
  } catch {
    closeOk = false;
  }
  const stderr = srv.stderr();
  const fatalAt = srv.firstLineAt("[server] FATAL class=");

  check(`FC-01 ${inject}：supervisor 观察到 worker 终止`,
    fatalAt !== null && stderr.includes(expectDetail),
    `FATAL 行到达时间=${fatalAt ?? "从未出现"}，期望 detail 含「${expectDetail}」`);
  check(`FC-03 ${inject}：进程在 ${EXIT_TIMEOUT_MS} ms 内非零退出（不是无限期存活）`,
    !exit.timedOut, exit.timedOut ? "进程未退出：这正是 zombie 形态（listener 存活、状态冻结）" : `退出耗时在界内`);
  check(`FC-03 ${inject}：退出码恰为 70`, exit.code === 70, `exit=${exit.code} signal=${exit.signal}`);
  check(`FC-04 ${inject}：stderr 给出致命类别 ${expectClass}`,
    stderr.includes(`class=${expectClass}`), `期望 class=${expectClass}`);
  check(`FC-05 ${inject}：stderr 不含会话令牌`, !stderr.includes(token),
    `令牌长度 ${token.length}，stderr ${stderr.length} 字节`);
  check(`FC-06 ${inject}：已连接的 WS 客户端失去连接`, closeOk && framesOk && obs.frames >= 1,
    `frames=${obs.frames} closeSeen=${obs.closeSeen} closeCode=${obs.closeCode}`);
  const zombieMs = obs.lastFrameAt !== null && exit.at ? exit.at - obs.lastFrameAt : null;
  check(`FC-02 ${inject}：zombie 窗口有上界（≤ ${ZOMBIE_BOUND_MS} ms）`,
    zombieMs !== null && zombieMs >= 0 && zombieMs <= ZOMBIE_BOUND_MS, `实测 ${zombieMs} ms`);
  srv.stop();
  return { zombieMs, frames: obs.frames, closeCode: obs.closeCode };
}

/**
 * 场景 C：把降级窗口拉长到可观测的量级，直接检验「不保持假健康」。
 *
 * 判定的顺序性质：致命上报先于延迟，因此在延迟期间请求必须已一律 503——
 * 这正是产品在真实故障下的语义（真实运行中该窗口是微秒级，观测不到而已）。
 */
async function scenarioDegradedWindow({ trace, webRoot }) {
  const srv = await startFaultyServer({ trace, webRoot, inject: "source-panic", holdMs: HOLD_MS });
  const ready = await waitReady(srv.port, srv);
  if (!ready.ok) {
    srv.stop();
    check("降级窗口场景就绪", false, `${ready.last} exitCode=${ready.exitCode}`);
    return;
  }
  const token = ready.token;
  const obs = openWs(srv.port, token);
  // 以下三次等待全部用不抛版本：本场景要检验的正是「降级是否发生」，
  // 若产品根本没有进入降级，那应当记为一条 FAIL 判据，而不是让脚本以
  // HARNESS FAILURE(4) 结束——judge 契约里 1 与 4 含义不同，把产品失败
  // 报成 harness 失败会让观测方去查测试而不是查产品。
  const gotFrame = await waitForSoft(() => obs.frames >= 1, 20_000);
  const gotFatal = await waitForSoft(() => srv.firstLineAt("[server] FATAL class=") !== null, 10_000);
  check("降级窗口前置：客户端已收到帧且服务端已进入降级",
    gotFrame && gotFatal, `frames=${obs.frames} 致命行=${gotFatal}`);
  const exitCodeAtWindowTime = srv.proc.exitCode;
  const aliveDuringWindow = exitCodeAtWindowTime === null;
  const snap = await authedGet(srv.port, "/api/snapshot", token);
  const boot = await authedGet(srv.port, "/api/bootstrap", token);
  const closedDuringWindow = obs.closeSeen;
  const exit = await waitExit(srv);
  const stderr = srv.stderr();
  const holdAt = srv.firstLineAt("FATAL hold=");

  check("FC-02 降级窗口内进程仍存活（说明窗口确实被拉长，503 不是竞态抽样）",
    aliveDuringWindow, `窗口内 exitCode=${exitCodeAtWindowTime}`);
  check("FC-02 降级窗口内 /api/snapshot 返回 503（不得按健康语义回答）",
    snap.status === 503, `status=${snap.status} body=${snap.body ?? snap.error}`);
  check("FC-02 降级窗口内 /api/bootstrap 返回 503", boot.status === 503,
    `status=${boot.status} body=${boot.body ?? boot.error}`);
  check("FC-06 降级窗口内已连接的 WS 客户端已被断开", closedDuringWindow,
    `closeCode=${obs.closeCode}`);
  check("FC-03 带延迟运行时进程仍按界退出", !exit.timedOut, `timedOut=${exit.timedOut}`);
  check("FC-03 带延迟运行时退出码仍为 70", exit.code === 70, `exit=${exit.code}`);
  check("FC-05 带延迟运行 stderr 不含会话令牌", !stderr.includes(token));
  const windowMs = holdAt !== null && exit.at ? exit.at - holdAt : null;
  check("延迟确实生效（窗口时长 ≥ 1 s，判定顺序未被延迟掩盖）",
    windowMs !== null && windowMs >= 1000, `实测 ${windowMs} ms`);
  srv.stop();
}

/**
 * 前置断言：被测 debug 二进制必须真的编译进了注入钩子。
 *
 * 若把 release 二进制交给本脚本，注入被忽略、服务器会一直正常运行——那会表现为
 * 「等待超时」。此处把它变成一条明确的失败原因，而不是让观测方去猜超时含义。
 */
async function assertInjectionCompiledIn() {
  if (!existsSync(NAV_CLI)) {
    throw new Error(`nav-core-cli 不存在：${NAV_CLI}（请先 cargo build）`);
  }
  const bytes = await readFile(NAV_CLI);
  const has = (s) => bytes.includes(Buffer.from(s, "ascii"));
  const ok = has(INJECT_ENV) && has(HOLD_ENV);
  check("前置：被测二进制编译进了故障注入钩子（debug 构建）", ok,
    ok ? `${NAV_CLI} 含 ${INJECT_ENV} / ${HOLD_ENV}`
      : `${NAV_CLI} 不含注入钩子——release 产物不得用于本套件`);
  if (!ok) {
    console.error("HARNESS FAILURE: 注入钩子缺失，后续断言全部无意义。");
    process.exit(4);
  }
}

async function main() {
  if (existsSync(WORK_DIR)) rmSync(WORK_DIR, { recursive: true, force: true });
  mkdirSync(WORK_DIR, { recursive: true });
  const webRoot = join(WORK_DIR, "web");
  mkdirSync(webRoot, { recursive: true }); // 本套件只走 /api 与 /ws，不需要前端产物
  console.log("=== nav-server 数据源故障遏制（P4R Batch 6A §4）===");
  console.log(`nav-core-cli : ${NAV_CLI}`);
  console.log(`dataset      : ${DATASET}`);
  await assertInjectionCompiledIn();
  const trace = await prepareTrace();
  console.log(`trace        : ${trace} (${statSync(trace).size} B)`);

  console.log("");
  console.log("--- 场景 A：worker panic → supervisor → 非零退出（重复以取上界分布）---");
  const zombies = [];
  for (let i = 0; i < PANIC_REPEATS; i++) {
    const r = await scenario({
      trace, webRoot, inject: "source-panic", expectClass: "SOURCE_WORKER_PANIC",
      expectDetail: "[fault-inject]",
    });
    if (r) zombies.push(r.zombieMs);
  }
  const st = stats(zombies);
  console.log(`  zombie 窗口（ms）：min=${st.min} median=${st.median} p95=${st.p95} max=${st.max} n=${st.n}`);

  console.log("");
  console.log("--- 场景 B：worker 意外正常返回 → 同样判为致命 ---");
  await scenario({
    trace, webRoot, inject: "source-exit", expectClass: "SOURCE_WORKER_EXIT",
    expectDetail: "数据源循环在无致命信号的情况下返回",
  });

  console.log("");
  console.log("--- 场景 C：降级窗口（测试专用延迟）内不得保持假健康 ---");
  await scenarioDegradedWindow({ trace, webRoot });

  const failed = results.filter((r) => !r.ok);
  console.log("");
  console.log(`=== 断言合计 ${results.length}，失败 ${failed.length} ===`);
  for (const f of failed) console.log(`  FAIL ${f.name} — ${f.detail}`);
  if (failed.length > 0) {
    console.log("FAULT CONTAINMENT: FAIL");
    return 1;
  }
  console.log("FAULT CONTAINMENT: PASS");
  console.log(`（本脚本捕获的 stderr 片段均经 redact；未打印任何令牌）${redact("")}`);
  return 0;
}

main()
  .then((code) => {
    try { rmSync(WORK_DIR, { recursive: true, force: true }); } catch { /* 清理失败不影响判定 */ }
    process.exit(code);
  })
  .catch((e) => {
    console.error(`HARNESS FAILURE: ${e?.stack ?? e}`);
    process.exit(4);
  });
