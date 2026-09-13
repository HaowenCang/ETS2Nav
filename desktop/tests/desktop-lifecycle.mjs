// ETS2Nav Desktop 生命周期验证（P4R Batch 6A §19）。
//
// 被判定的性质：
//   D-01 正常启动：sidecar 身份校验 → 启动 → 就绪 → 可服务（bootstrap/WS/路线请求）
//   D-02 dataset 缺失：明确错误、非零退出、**不启动 sidecar、不开窗口**
//   D-03 sidecar bind/启动失败：明确错误、非零退出
//   D-04 正常退出：sidecar 被回收
//   D-05 Desktop 消失后无 orphan nav-core-cli（含被强杀的情形）
//   D-06 sidecar 意外退出：Desktop 能检测（退出码 23）
//   D-07 sidecar fatal：不得留下「已连接但状态冻结」的界面（无窗口模式下表现为立即退出）
//
// 被测对象是**打包产物**（bundle 目录里的 ets2nav-desktop.exe），不是
// desktop/target 下的开发产物：产品边界只有在随包布局下才成立。
//
// 退出码 0 = 全部 PASS；1 = 有 FAIL；3 = 前置条件不足（bundle 不存在）；
// 4 = harness 自身错误。绝不因「未执行」而退出 0。
//
// 本脚本不打印令牌。

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const args = process.argv.slice(2);
function argValue(name, dflt = null) {
  const i = args.indexOf(name);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : dflt;
}
const argFlag = (name) => args.includes(name);

const BUNDLE = argValue("--bundle");
const WORK = join(tmpdir(), "ets2nav-desktop-lifecycle");
const FAIL = [];
let passCount = 0;

function check(name, ok, detail = "") {
  if (ok) {
    passCount += 1;
    console.log(`  [PASS] ${name}${detail ? ` - ${detail}` : ""}`);
  } else {
    FAIL.push(name);
    console.log(`  [FAIL] ${name}${detail ? ` - ${detail}` : ""}`);
  }
}
function notVerified(name, why) {
  console.log(`  [NOT VERIFIED] ${name} - ${why}`);
}

function tasklistHas(pid) {
  try {
    const out = execFileSync("tasklist", ["/FI", `PID eq ${pid}`, "/NH", "/FO", "CSV"], {
      encoding: "utf8", windowsHide: true,
    });
    return new RegExp(`"${pid}"`).test(out);
  } catch {
    return false;
  }
}

/** 解析 stdout 生命周期行：`[desktop] key=value ...` → {key: value} 列表。 */
function parseLifecycle(stdout) {
  const rec = { raw: stdout, fields: {} };
  for (const line of stdout.split(/\r?\n/)) {
    // 键名允许下划线（page_load / sidecar-started 都要能解析）。
    const m = /^\[desktop\] ([a-z0-9_-]+)(?: (.*))?$/.exec(line.trim());
    if (!m) continue;
    const rest = m[2] ?? "";
    const kv = {};
    for (const tok of rest.split(/\s+/)) {
      const eq = tok.indexOf("=");
      if (eq > 0) kv[tok.slice(0, eq)] = tok.slice(eq + 1);
    }
    rec.fields[m[1]] = kv;
    (rec[m[1]] ??= []).push(kv);
  }
  return rec;
}

function runDesktop({ extraArgs = [], env = {}, timeoutMs = 180_000 }) {
  const exe = join(BUNDLE, "ets2nav-desktop.exe");
  const proc = spawn(exe, extraArgs, {
    cwd: BUNDLE,
    stdio: ["ignore", "pipe", "pipe"],
    windowsHide: true,
    env: { ...process.env, ...env },
  });
  let stdout = "", stderr = "";
  proc.stdout.on("data", (d) => { stdout += d; });
  proc.stderr.on("data", (d) => { stderr += d; });
  const exited = new Promise((ok) => proc.once("close", (code, signal) => ok({ code, signal })));
  const timedOut = new Promise((ok) => setTimeout(() => ok({ code: "TIMEOUT" }), timeoutMs));
  return {
    proc,
    get stdout() { return stdout; },
    get stderr() { return stderr; },
    finish: () => Promise.race([exited, timedOut]),
  };
}

/** 等到 stdout 出现某条生命周期记录（轮询 stdout 缓冲，非产品行为）。 */
async function waitForField(run, key, timeoutMs = 120_000) {
  return waitForFieldValue(run, key, () => true, timeoutMs);
}

/**
 * 等到 stdout 出现满足 `pred` 的某条记录。
 *
 * 必须能按**取值**等待而不是只按键等待：`page_load` 会先出现 `state=Started` 再出现
 * `state=Finished`，只按键等待会拿到 Started 就返回，于是「页面加载完成」这条断言
 * 变成对加载开始的断言——判据与语义不符，而且后续依赖「UI 已运行」的检查会过早执行。
 */
async function waitForFieldValue(run, key, pred, timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    for (const kv of parseLifecycle(run.stdout)[key] ?? []) {
      if (pred(kv)) return kv;
    }
    if (run.proc.exitCode !== null) return null;
    await new Promise((r) => setTimeout(r, 50));
  }
  return null;
}

async function prepareTrace(dataset) {
  const out = join(WORK, "desktop.navtrace");
  if (existsSync(out)) return out;
  const navCli = join(BUNDLE, "nav-core-cli.exe");
  await new Promise((ok, bad) => {
    const p = spawn(navCli, ["syntrace", "-58456,32832:-52925,36510", dataset, out],
      { stdio: ["ignore", "pipe", "pipe"], cwd: BUNDLE, windowsHide: true });
    let err = "";
    p.stderr.on("data", (d) => { err += d; });
    p.on("error", bad);
    p.on("close", (code) => (code === 0 && existsSync(out) ? ok() : bad(new Error(`syntrace exit=${code} ${err}`))));
  });
  return out;
}

function openWs(port, token) {
  const ws = new WebSocket(`ws://127.0.0.1:${port}/ws?token=${token}`);
  const obs = { frames: 0, mapStates: 0, closed: false, closeCode: null, bootstrap: null };
  ws.addEventListener("message", (e) => {
    obs.frames += 1;
    try {
      const d = JSON.parse(e.data);
      if (d && d.type === "map_state") obs.mapStates += 1;
      if (d && d.type === "vehicle") obs.bootstrap = d;
    } catch { /* 非 JSON 帧不计入语义统计，但计入帧数 */ }
  });
  ws.addEventListener("close", (e) => { obs.closed = true; obs.closeCode = e.code; });
  ws.addEventListener("error", () => {});
  obs.ws = ws;
  return obs;
}

async function main() {
  if (!BUNDLE || !existsSync(join(BUNDLE, "ets2nav-desktop.exe"))) {
    console.error(`PRECONDITION FAILURE: bundle not found at ${BUNDLE}（先运行 desktop/scripts/assemble-bundle.ps1）`);
    return 3;
  }
  if (existsSync(WORK)) rmSync(WORK, { recursive: true, force: true });
  mkdirSync(WORK, { recursive: true });
  const dataset = join(BUNDLE, "data", "europe-v5");
  console.log("=== ETS2Nav Desktop 生命周期（P4R Batch 6A §19）===");
  console.log(`bundle  : ${resolve(BUNDLE)}`);
  console.log(`dataset : ${dataset}`);

  const trace = await prepareTrace(dataset);

  // ── D-01 ─────────────────────────────────────────────────────────────────
  console.log("\n-- D-01 正常启动：身份 → sidecar → 就绪 → 可服务 --");
  const d01 = runDesktop({
    extraArgs: [
      "--no-window", "--hold-ms=9000",
      `--replay=${trace}`, "--fake-signal",
    ],
  });
  const started = await waitForField(d01, "sidecar-started", 150_000);
  const serverLine = await waitForField(d01, "server", 150_000);
  const parsed = parseLifecycle(d01.stdout);
  check("D-01 sidecar 身份被校验为 verified",
    parsed.fields.sidecar?.identity === "verified",
    `identity=${parsed.fields.sidecar?.identity ?? "缺失"}`);
  check("D-01 sidecar 进程已启动且记录 pid", started !== null && Number(started.pid) > 0,
    `pid=${started?.pid ?? "缺失"}`);
  const port = serverLine ? Number(serverLine.port) : null;
  check("D-01 端口经 stdout 通道协商（--port=0 → 实际端口）",
    port !== null && port > 0 && port !== 8123, `port=${port}`);
  check("D-01 sidecar 就绪判据为 bootstrap 200（ready_ms 有记录）",
    serverLine !== undefined && Number(serverLine?.ready_ms) > 0, `ready_ms=${serverLine?.ready_ms}`);

  let boot = null, obs = null;
  if (port) {
    const r = await fetch(`http://127.0.0.1:${port}/api/bootstrap`, { signal: AbortSignal.timeout(5000) });
    boot = r.ok ? await r.json() : { status: r.status };
    check("D-01 UI 所需引导可用（bootstrap 200 且返回 64 位令牌）",
      r.status === 200 && typeof boot.token === "string" && boot.token.length === 64,
      `status=${r.status}`);
    if (boot?.token) {
      obs = openWs(port, boot.token);
      const deadline = Date.now() + 20_000;
      while (Date.now() < deadline && obs.frames < 3 && d01.proc.exitCode === null) {
        await new Promise((r2) => setTimeout(r2, 50));
      }
      check("D-01 WS 连接建立并收到实时帧（快照可见）", obs.frames >= 1, `frames=${obs.frames}`);
    }
    // 路线请求：POST /api/route 之后服务端应广播 map_state（§60）
    if (boot?.token) {
      const body = JSON.stringify({ from: [-58456, 32832], to: [-52925, 36510] });
      const rr = await fetch(`http://127.0.0.1:${port}/api/route`, {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Bearer ${boot.token}` },
        body,
        signal: AbortSignal.timeout(10_000),
      });
      const deadline = Date.now() + 15_000;
      while (Date.now() < deadline && obs.mapStates === 0 && d01.proc.exitCode === null) {
        await new Promise((r2) => setTimeout(r2, 50));
      }
      check("D-01 路线请求可用且触发 map_state 广播",
        rr.status === 200 && obs.mapStates >= 1,
        `http=${rr.status} map_state=${obs.mapStates}`);
    }
  }

  const d01Exit = await d01.finish();
  const d01Fields = parseLifecycle(d01.stdout);
  const sidecarPid = Number(d01Fields.fields["sidecar-started"]?.pid ?? 0);
  check("D-01 正常停机并回收 sidecar",
    d01Exit.code === 0
      && d01Fields.fields.shutdown?.reason === "no-window"
      && d01Fields.fields.shutdown?.reaped === "true"
      && d01Fields.fields.shutdown?.sidecar_running === "false",
    `exit=${d01Exit.code} shutdown=${JSON.stringify(d01Fields.fields.shutdown)}`);
  check("D-01 Desktop 报告的 pid 与实际运行的 sidecar 一致",
    sidecarPid > 0, `pid=${sidecarPid}`);

  // ── D-04 / D-05 ──────────────────────────────────────────────────────────
  console.log("\n-- D-04 / D-05 退出后不得残留 sidecar --");
  await new Promise((r) => setTimeout(r, 800));
  check("D-04 正常退出后 sidecar 进程已消失", sidecarPid > 0 && !tasklistHas(sidecarPid),
    `pid=${sidecarPid} 仍存在=${sidecarPid > 0 ? tasklistHas(sidecarPid) : "n/a"}`);

  // 强杀形态：作业对象（KILL_ON_JOB_CLOSE）必须在 Desktop 被强杀时也回收 sidecar。
  // 刻意不用「延时自动 kill」：等待逻辑会在 kill 触发前先返回，从而把「强杀」悄悄
  // 变成「等它自己正常退出」——那样的通过是空洞的。此处显式分三步：确认存活 →
  // 强杀 → 确认回收。
  const d05 = runDesktop({ extraArgs: ["--no-window", "--hold-ms=60000", `--replay=${trace}`] });
  const d05Started = await waitForField(d05, "sidecar-started", 150_000);
  const d05Pid = Number(d05Started?.pid ?? 0);
  await new Promise((r) => setTimeout(r, 1500));
  const d05AliveBefore = d05Pid > 0 && tasklistHas(d05Pid);
  check("D-05 强杀前 sidecar 确实在运行（前置，防止空洞断言）",
    d05AliveBefore, `pid=${d05Pid} 强杀前存在=${d05AliveBefore}`);
  try { d05.proc.kill("SIGKILL"); } catch { /* 已退出 */ }
  const d05Exit = await d05.finish();
  await new Promise((r) => setTimeout(r, 1500));
  const d05AliveAfter = d05Pid > 0 && tasklistHas(d05Pid);
  // Windows 上 TerminateProcess 表现为退出码而非 signal，因此判据是
  // 「非正常退出（code≠0，或根本没有正常收尾行）」而不是 signal 名。
  const d05Abnormal = d05Exit.code !== 0 || !/\[desktop\] exit/.test(d05.stdout);
  check("D-05 Desktop 被强杀后 sidecar 仍被回收（作业对象保证）",
    d05AliveBefore && !d05AliveAfter && d05Abnormal,
    `pid=${d05Pid} 强杀后存在=${d05AliveAfter} desktop_exit=${d05Exit.code} 非正常退出=${d05Abnormal}`);

  // ── D-02 ─────────────────────────────────────────────────────────────────
  console.log("\n-- D-02 dataset 缺失：明确失败，不得开空白窗口 --");
  const emptyDataset = join(WORK, "empty-dataset");
  mkdirSync(emptyDataset, { recursive: true });
  const d02 = runDesktop({ extraArgs: ["--no-window", "--hold-ms=1000", `--dataset=${emptyDataset}`] });
  const d02Exit = await d02.finish();
  check("D-02 数据集缺失时退出码为 21", d02Exit.code === 21, `exit=${d02Exit.code}`);
  check("D-02 stderr 给出 dataset 类错误",
    /kind=dataset-/.test(d02.stderr), d02.stderr.trim().split(/\r?\n/).slice(-1)[0] ?? "");
  check("D-02 未启动 sidecar、未协商端口（不存在『窗口开着但连不上』）",
    !parseLifecycle(d02.stdout).fields.server && !parseLifecycle(d02.stdout).fields["sidecar-started"]);

  // ── D-03 ─────────────────────────────────────────────────────────────────
  console.log("\n-- D-03 sidecar 启动失败：明确失败 --");
  const brokenDataset = join(WORK, "broken-dataset");
  mkdirSync(brokenDataset, { recursive: true });
  for (const f of ["manifest.json", "routing.graph", "junction.graph", "search.db"]) {
    writeFileSync(join(brokenDataset, f), "not-a-real-dataset\n");
  }
  const d03 = runDesktop({ extraArgs: ["--no-window", "--hold-ms=1000", `--dataset=${brokenDataset}`] });
  const d03Exit = await d03.finish();
  check("D-03 sidecar 无法加载数据集时退出码为 22", d03Exit.code === 22, `exit=${d03Exit.code}`);
  check("D-03 stderr 给出 sidecar-start 类错误",
    /kind=sidecar-start/.test(d03.stderr), d03.stderr.trim().split(/\r?\n/).slice(-1)[0] ?? "");

  // ── D-06 / D-07 ──────────────────────────────────────────────────────────
  console.log("\n-- D-06 / D-07 sidecar 运行期意外退出：Desktop 必须检测 --");
  // 主判据用**外部终止**而不是故障注入：它是与发布形态无关的真实死亡方式
  // （崩溃、被任务管理器结束、被 OOM killer 收割），因此 release bundle 同样可测。
  // 注入变体只在 sidecar 编译进钩子时（debug bundle）另跑，见下方。
  const d06 = runDesktop({ extraArgs: ["--no-window", "--hold-ms=30000", `--replay=${trace}`] });
  const d06Started = await waitForField(d06, "sidecar-started", 150_000);
  const d06Pid = Number(d06Started?.pid ?? 0);
  // 必须等 sidecar 真正就绪再杀：在启动竞态里杀它测的是 D-03（启动失败）而不是 D-06。
  const d06Ready = await waitForField(d06, "server", 150_000);
  check("D-06 前置：sidecar 已就绪后才终止它", d06Pid > 0 && d06Ready !== null,
    `pid=${d06Pid} port=${d06Ready?.port}`);
  try {
    execFileSync("taskkill", ["/PID", String(d06Pid), "/F"], { windowsHide: true });
  } catch { /* 已退出 */ }
  const d06Exit = await d06.finish();
  const d06Fields = parseLifecycle(d06.stdout);
  check("D-06 sidecar 运行期被终止时 Desktop 检测到（退出码 23）",
    d06Exit.code === 23, `exit=${d06Exit.code}`);
  check("D-06 stderr 记录 sidecar 退出，而不是静默停摆",
    /kind=sidecar-(exited|died-during-hold)/.test(d06.stderr),
    d06.stderr.trim().split(/\r?\n/).slice(-1)[0] ?? "");
  check("D-07 检测到 fatal 后立即停机（未等满 hold，界面不会停在冻结状态）",
    d06Fields.fields.shutdown !== undefined
      && /sidecar-died/.test(d06Fields.fields.shutdown?.reason ?? ""),
    `shutdown=${JSON.stringify(d06Fields.fields.shutdown)}`);

  // 注入变体（panic → 退出码 70）。只有 debug bundle 才有钩子：release 里连环境变量
  // 名字面量都不存在（这正是 §4 要的性质）。此处如实区分「跑了」与「本形态不可跑」，
  // 不把不可跑写成通过——panic 路径本身由 FaultContainment 套件在 debug 二进制上断言。
  const hookPresent = readFileSync(join(BUNDLE, "nav-core-cli.exe"))
    .includes(Buffer.from("ETS2NAV_FAULT_INJECT", "ascii"));
  if (hookPresent) {
    const d06p = runDesktop({
      extraArgs: ["--no-window", "--hold-ms=30000", `--replay=${trace}`],
      env: { ETS2NAV_FAULT_INJECT: "source-panic" },
    });
    await waitForField(d06p, "sidecar-started", 150_000);
    const d06pExit = await d06p.finish();
    check("D-06p sidecar panic（退出码 70）同样被 Desktop 检测为 23",
      d06pExit.code === 23, `exit=${d06pExit.code}`);
    check("D-06p stderr 记录 sidecar 退出码 70",
      /sidecar_exit=70/.test(d06p.stderr),
      d06p.stderr.trim().split(/\r?\n/).slice(-1)[0] ?? "");
  } else {
    notVerified("D-06p sidecar panic 变体",
      "本 bundle 的 sidecar 不含故障注入钩子（release 构建的正常结果）；"
      + "panic → 非零退出的路径由 FaultContainment 套件在 debug 二进制上以 FC-01…FC-06 断言");
  }

  // ── 窗口形态（可选，需交互式桌面）────────────────────────────────────────
  console.log("\n-- 窗口形态（--windowed 时才执行）--");
  if (!argFlag("--windowed")) {
    notVerified("D-01w/D-04w 真实 WebView 窗口路径",
      "未传 --windowed（headless 环境无法验证窗口）；窗口形态由本机另行人工运行验证");
  } else {
    const w = runDesktop({ extraArgs: [`--replay=${trace}`, "--fake-signal"] });
    const wServer = await waitForField(w, "server", 150_000);
    const wWindow = await waitForField(w, "window", 60_000);
    // 等 Finished 而不是等 page_load 这个键出现（Started 会先到）。
    const wLoad = await waitForFieldValue(w, "page_load", (kv) => kv.state === "Finished", 60_000);
    check("D-01w 窗口被创建", wWindow !== null, JSON.stringify(wWindow));
    check("D-01w WebView 完成页面加载（page_load=Finished）",
      wLoad !== null && wLoad.state === "Finished", JSON.stringify(wLoad));
    // WebView 自身的 WS 连接可由服务端观测：/api/metadata 的 ws_clients。
    // 它同时是「UI 的 JS 真的运行了」的证据——页面加载完成只说明资源到位，
    // 说明 app.js 跑起来并完成 bootstrap 的是这条连接。
    if (wServer?.port) {
      const b = await fetch(`http://127.0.0.1:${wServer.port}/api/bootstrap`);
      const bd = await b.json();
      let clients = 0;
      const deadline = Date.now() + 30_000;
      while (Date.now() < deadline && clients < 1) {
        const md = await fetch(`http://127.0.0.1:${wServer.port}/api/metadata`, {
          headers: { Authorization: `Bearer ${bd.token}` },
        });
        const mdj = md.ok ? await md.json() : {};
        clients = Number(mdj.ws_clients ?? 0);
        if (clients < 1) await new Promise((r2) => setTimeout(r2, 200));
      }
      check("D-01w WebView 内的 UI 已连上 WS（服务端观测到客户端连接）",
        clients >= 1, `ws_clients=${clients}`);
    }
    // 关闭窗口：通过任务栏关闭不可脚本化，此处以强杀验证「窗口消失即回收」
    w.proc.kill("SIGKILL");
    await w.finish();
  }

  console.log("");
  if (FAIL.length > 0) {
    console.log(`DESKTOP LIFECYCLE: FAIL (${FAIL.length}) - ${FAIL.join("; ")}`);
    return 1;
  }
  console.log(`DESKTOP LIFECYCLE: PASS (${passCount} checks)`);
  return 0;
}

main()
  .then((code) => {
    try { rmSync(WORK, { recursive: true, force: true }); } catch { /* 清理失败不影响判定 */ }
    process.exit(code);
  })
  .catch((e) => {
    console.error(`HARNESS FAILURE: ${e?.stack ?? e}`);
    process.exit(4);
  });
