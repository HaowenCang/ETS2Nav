// ETS2Nav Desktop 作业对象 fail-closed 判定（P4R Batch 6B §4）。
//
// ## 被判定的性质
//
// J-01 正常 release 产物：作业对象被成功接管（`state=assigned required=true`）。
// J-02 发布产物不含注入面：`ets2nav-desktop.exe` 中不存在 `ETS2NAV_JOB_FAULT` 字面量。
// J-03 注入构建**确实**带有钩子（J-02 的正对照：否则 J-04/J-05 可能因为钩子根本没编译
//      进去而"通过"——那种通过是空洞的）。
// J-04 release + create 注入失败 → fail closed：退出码 24、`step=create`、**不开窗口**、
//      非零退出。create 发生在派生之前，因此不存在需要回收的子进程。
// J-05 release + assign 注入失败 → fail closed：退出码 24、`step=assign`、**不开窗口**、
//      且已派生的 sidecar 被立即回收（无残留 nav-core-cli 进程）。
// J-06 显式降级（`--allow-no-job-object`）→ 允许继续，但必须留下 WARN 且状态可观测。
//      release 与 debug 两种 profile 都测：策略由调用方声明决定，不由 `debug_assertions`
//      决定，因此两种 profile 必须给出同一判定。
//
// ## 为什么注入构建必须单独存在
//
// 被判定的性质是「**release 构建**在作业对象不可用时 fail closed」。若注入只在 debug
// 构建中存在，release 侧的判据就只能靠阅读代码推断——而「编译通过不等于行为成立」
// 正是本批次要消除的推理方式。`--features fault-inject` 使 release 判据可被真正执行，
// 同时默认构建（发布用）不含任何注入代码。
//
// ## 被判定的对象
//
// J-01/J-02 用**随包产物**（bundle 目录）。J-04…J-06 需要注入构建，而复制 356 MB 数据集
// 只为换一个可执行文件是浪费：改为把注入 exe 放进一个只含 `bundle-manifest.json` 的临时
// 目录，再用 `--dataset/--web/--sidecar` 指向真实 bundle 的内容。这样"被测试的二进制"是
// 注入构建，而它周围的 bundle 内容仍是发布产物——本文件判定的是作业对象策略，不是
// bundle 内容身份（后者由 desktop-lifecycle.mjs 与 verify-bundle.ps1 判定）。
//
// 退出码：0 全部 PASS；1 有 FAIL；3 前置条件不足（未提供必要路径）；4 harness 自身错误。
// 绝不因「未执行」而退出 0。本脚本不打印令牌。

import { execFileSync, spawn } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const args = process.argv.slice(2);
const argValue = (name, dflt = null) => {
  const i = args.indexOf(name);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : dflt;
};

const BUNDLE = argValue("--bundle");
const INJECT_RELEASE = argValue("--inject-exe-release");
const INJECT_DEBUG = argValue("--inject-exe-debug");
const WORK = join(tmpdir(), "ets2nav-job-object-gate");

const FAIL = [];
const NOT_VERIFIED = [];
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
  NOT_VERIFIED.push(name);
  console.log(`  [NOT VERIFIED] ${name} - ${why}`);
}

/** 按镜像名列出当前 pid 集合。用于判定「是否残留 sidecar」。 */
function pidsOf(imageName) {
  try {
    const out = execFileSync(
      "tasklist",
      ["/FI", `IMAGENAME eq ${imageName}`, "/NH", "/FO", "CSV"],
      { encoding: "utf8", windowsHide: true },
    );
    const pids = new Set();
    for (const line of out.split(/\r?\n/)) {
      const m = /^"([^"]+)","(\d+)"/.exec(line.trim());
      if (m && m[1].toLowerCase() === imageName.toLowerCase()) pids.add(Number(m[2]));
    }
    return pids;
  } catch {
    return new Set();
  }
}

function parseLifecycle(stdout) {
  const rec = { raw: stdout, fields: {} };
  for (const line of stdout.split(/\r?\n/)) {
    const m = /^\[desktop\] ([a-z0-9_-]+)(?: (.*))?$/.exec(line.trim());
    if (!m) continue;
    const kv = {};
    for (const tok of (m[2] ?? "").split(/\s+/)) {
      const eq = tok.indexOf("=");
      if (eq > 0) kv[tok.slice(0, eq)] = tok.slice(eq + 1);
    }
    rec.fields[m[1]] = kv;
    (rec[m[1]] ??= []).push(kv);
  }
  return rec;
}

/**
 * 先证明 stdout 通道确实被捕获，再允许对该运行的**否定式**断言成立。
 *
 * 没有这一步，「未出现 window」「未出现 sidecar-started」这类断言在 stdout 为空时会
 * 恒真——测试会在一片空输出上全绿。这不是假想：本文件首版就漏填了 `fields`，于是
 * J-04/J-05 的否定断言全部空洞通过。捕获失败必须表现为失败，而不是表现为「什么都没
 * 发生，所以什么都没出错」。
 */
function assertStdoutCaptured(label, run) {
  const sawStart = /\[desktop\] start /.test(run.stdout);
  check(`${label} 前置：stdout 通道确实被捕获（否定式断言不得在空输出上成立）`,
    sawStart, `stdout_bytes=${run.stdout.length}`);
  return sawStart;
}

/**
 * 运行一个 Desktop 可执行文件。
 *
 * `noWindow=false` 时**刻意不传** `--no-window`：被判定的性质之一是「fail closed 时不开
 * 窗口」，而在 `--no-window` 下窗口本来就不会被创建，那条断言会退化成恒真。因此真实
 * fail-closed 用例必须跑在会开窗口的配置下，并断言 `[desktop] window` 从未出现。
 * 若策略被写坏（作业对象失败却继续启动），进程会真的开出窗口并挂住——超时保护负责
 * 把它杀掉并把用例判为失败。
 */
function runDesktop({
  exe,
  extraArgs = [],
  env = {},
  timeoutMs = 60_000,
  stdio = ["ignore", "pipe", "pipe"],
}) {
  const proc = spawn(exe, extraArgs, {
    cwd: WORK,
    stdio,
    windowsHide: true,
    env: { ...process.env, ...env },
  });
  let stdout = "";
  let stderr = "";
  if (proc.stdout) proc.stdout.on("data", (d) => { stdout += d; });
  if (proc.stderr) proc.stderr.on("data", (d) => { stderr += d; });
  const exited = new Promise((ok) =>
    proc.once("close", (code, signal) => ok({ code, signal, timedOut: false })));
  let timer = null;
  const timedOut = new Promise((ok) => {
    timer = setTimeout(() => {
      try { proc.kill("SIGKILL"); } catch { /* 已退出 */ }
      ok({ code: "TIMEOUT", signal: null, timedOut: true });
    }, timeoutMs);
  });
  const finish = async () => {
    const r = await Promise.race([exited, timedOut]);
    if (timer) clearTimeout(timer);
    return r;
  };
  return { proc, get stdout() { return stdout; }, get stderr() { return stderr; }, finish };
}

/** 建立只含清单的临时 bundle 目录，使注入 exe 能在「有有效清单」的前提下运行。 */
function makeInjectionRoot(name, exe, manifestPath) {
  const dir = join(WORK, name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  copyFileSync(exe, join(dir, "ets2nav-desktop.exe"));
  copyFileSync(manifestPath, join(dir, "bundle-manifest.json"));
  return join(dir, "ets2nav-desktop.exe");
}

async function main() {
  if (!BUNDLE || !existsSync(join(BUNDLE, "ets2nav-desktop.exe"))) {
    console.error(
      `PRECONDITION FAILURE: 未找到 ${BUNDLE ?? "(未提供 --bundle)"}/ets2nav-desktop.exe（先运行 assemble-bundle.ps1）`,
    );
    return 3;
  }
  const manifestPath = join(BUNDLE, "bundle-manifest.json");
  if (!existsSync(manifestPath)) {
    console.error(`PRECONDITION FAILURE: 未找到 ${manifestPath}`);
    return 3;
  }
  const hasInjectRelease = INJECT_RELEASE && existsSync(INJECT_RELEASE);
  const hasInjectDebug = INJECT_DEBUG && existsSync(INJECT_DEBUG);
  if (!hasInjectRelease) {
    console.error(
      `PRECONDITION FAILURE: 未提供 --inject-exe-release 或文件不存在（${INJECT_RELEASE ?? "未提供"}）；`
      + "先运行：cargo build --release --features fault-inject --manifest-path desktop/Cargo.toml",
    );
    return 3;
  }

  if (existsSync(WORK)) rmSync(WORK, { recursive: true, force: true });
  mkdirSync(WORK, { recursive: true });

  const dataset = join(BUNDLE, "data", "europe-v5");
  const web = join(BUNDLE, "web");
  const sidecar = join(BUNDLE, "nav-core-cli.exe");
  const overrides = [
    `--dataset=${dataset}`,
    `--web=${web}`,
    `--sidecar=${sidecar}`,
  ];

  console.log("=== ETS2Nav Desktop 作业对象 fail-closed（P4R Batch 6B §4）===");
  console.log(`bundle         : ${resolve(BUNDLE)}`);
  console.log(`inject release : ${INJECT_RELEASE}`);
  console.log(`inject debug   : ${INJECT_DEBUG ?? "(未提供)"}`);

  // ── J-01 正常 release 产物：作业对象被接管 ────────────────────────────────
  console.log("\n-- J-01 正常 release：job=assigned --");
  const base = runDesktop({
    exe: join(BUNDLE, "ets2nav-desktop.exe"),
    extraArgs: ["--no-window", "--hold-ms=1200", ...overrides],
    timeoutMs: 120_000,
  });
  const baseExit = await base.finish();
  assertStdoutCaptured("J-01", base);
  const baseFields = parseLifecycle(base.stdout).fields;
  check("J-01 退出码为 0", baseExit.code === 0, `exit=${baseExit.code}`);
  check("J-01 sidecar-job state=assigned",
    baseFields["sidecar-job"]?.state === "assigned",
    `state=${baseFields["sidecar-job"]?.state ?? "缺失"}`);
  check("J-01 required=true（缺省即要求作业对象）",
    baseFields["sidecar-job"]?.required === "true",
    `required=${baseFields["sidecar-job"]?.required ?? "缺失"}`);
  check("J-01 启动行声明 job_required=true",
    baseFields.start?.job_required === "true",
    `job_required=${baseFields.start?.job_required ?? "缺失"}`);

  // ── J-02 / J-03 注入面：发布产物无、注入构建有 ────────────────────────────
  console.log("\n-- J-02 / J-03 注入面存在性 --");
  const LITERAL = "ETS2NAV_JOB_FAULT";
  const shippedHas = readFileSync(join(BUNDLE, "ets2nav-desktop.exe"))
    .includes(Buffer.from(LITERAL, "ascii"));
  const injectedHas = readFileSync(INJECT_RELEASE).includes(Buffer.from(LITERAL, "ascii"));
  check(`J-02 发布产物不含 '${LITERAL}' 字面量`, !shippedHas, `present=${shippedHas}`);
  check(`J-03 注入构建含 '${LITERAL}'（J-02 的正对照，防止空洞通过）`,
    injectedHas, `present=${injectedHas}`);

  const injectedExe = makeInjectionRoot("inject-release", INJECT_RELEASE, manifestPath);

  // ── J-04 create 注入失败 → fail closed（不派生、不开窗口）────────────────
  console.log("\n-- J-04 create 注入失败：fail closed，不开窗口 --");
  const j04 = runDesktop({
    exe: injectedExe,
    extraArgs: [...overrides, "--hold-ms=1200"],
    env: { ETS2NAV_JOB_FAULT: "create" },
  });
  const j04Exit = await j04.finish();
  assertStdoutCaptured("J-04", j04);
  const j04Fields = parseLifecycle(j04.stdout).fields;
  check("J-04 退出码为 24（sidecar-job-required）", j04Exit.code === 24, `exit=${j04Exit.code}`);
  check("J-04 stderr 记录 kind=sidecar-job-required 且 step=create",
    /kind=sidecar-job-required/.test(j04.stderr) && /step=create/.test(j04.stderr),
    j04.stderr.trim().split(/\r?\n/).filter((l) => l.includes("job-required")).slice(-1)[0] ?? "(无)");
  check("J-04 未创建窗口（fail closed 必须在建窗之前返回）",
    !j04Fields.window && !j04Fields.page_load,
    `window=${JSON.stringify(j04Fields.window ?? null)}`);
  check("J-04 未派生 sidecar（create 发生在 spawn 之前）",
    !j04Fields["sidecar-started"] && !j04Fields.server,
    `sidecar-started=${JSON.stringify(j04Fields["sidecar-started"] ?? null)}`);
  check("J-04 非零退出且不是超时", j04Exit.code !== 0 && !j04Exit.timedOut, `exit=${j04Exit.code}`);

  // ── J-05 assign 注入失败 → fail closed（已派生者必须被回收）──────────────
  console.log("\n-- J-05 assign 注入失败：fail closed，回收已派生的 sidecar --");
  const beforePids = pidsOf("nav-core-cli.exe");
  const j05 = runDesktop({
    exe: injectedExe,
    extraArgs: [...overrides, "--hold-ms=1200"],
    env: { ETS2NAV_JOB_FAULT: "assign" },
  });
  const j05Exit = await j05.finish();
  assertStdoutCaptured("J-05", j05);
  const j05Fields = parseLifecycle(j05.stdout).fields;
  check("J-05 退出码为 24（sidecar-job-required）", j05Exit.code === 24, `exit=${j05Exit.code}`);
  // step=assign 只有在 spawn 成功之后才可能被记录，因此它同时证明「确实派生过子进程」——
  // 没有这条，J-05 的回收断言会退化成「本来就没起过任何东西」。
  check("J-05 stderr 记录 step=assign（证明 spawn 已发生、assign 被尝试）",
    /step=assign/.test(j05.stderr),
    j05.stderr.trim().split(/\r?\n/).filter((l) => l.includes("job-required")).slice(-1)[0] ?? "(无)");
  check("J-05 stderr 含 AssignProcessToJobObject 的 win32 诊断",
    /AssignProcessToJobObject/.test(j05.stderr));
  check("J-05 未创建窗口", !j05Fields.window && !j05Fields.page_load,
    `window=${JSON.stringify(j05Fields.window ?? null)}`);
  await new Promise((r) => setTimeout(r, 1200));
  const afterPids = pidsOf("nav-core-cli.exe");
  const leaked = [...afterPids].filter((p) => !beforePids.has(p));
  check("J-05 已派生的 sidecar 被立即回收（无新增残留进程）",
    leaked.length === 0, `泄漏 pid=${JSON.stringify(leaked)}`);
  check("J-05 非零退出且不是超时", j05Exit.code !== 0 && !j05Exit.timedOut, `exit=${j05Exit.code}`);

  // ── J-06 显式降级：允许继续，但必须留下 WARN ─────────────────────────────
  console.log("\n-- J-06 显式降级 --allow-no-job-object：必须 WARN 且状态可观测 --");
  for (const [label, exe] of [
    ["release", injectedExe],
    ["debug", hasInjectDebug ? makeInjectionRoot("inject-debug", INJECT_DEBUG, manifestPath) : null],
  ]) {
    if (!exe) {
      notVerified(`J-06 ${label} 降级`,
        "未提供 --inject-exe-debug 或文件不存在；先运行 cargo build --features fault-inject --manifest-path desktop/Cargo.toml");
      continue;
    }
    const j06 = runDesktop({
      exe,
      extraArgs: ["--no-window", "--hold-ms=1200", "--allow-no-job-object", ...overrides],
      env: { ETS2NAV_JOB_FAULT: "create" },
      timeoutMs: 120_000,
    });
    const j06Exit = await j06.finish();
    assertStdoutCaptured(`J-06 ${label}`, j06);
    const j06Fields = parseLifecycle(j06.stdout).fields;
    check(`J-06 ${label} 降级后正常运行（退出码 0）`, j06Exit.code === 0, `exit=${j06Exit.code}`);
    check(`J-06 ${label} 留下 WARN kind=job-create-failed`,
      /WARN kind=job-create-failed/.test(j06.stderr),
      j06.stderr.trim().split(/\r?\n/).filter((l) => l.includes("WARN")).slice(-1)[0] ?? "(无)");
    check(`J-06 ${label} 状态可观测为 create-failed 且 required=false`,
      j06Fields["sidecar-job"]?.state === "create-failed"
        && j06Fields["sidecar-job"]?.required === "false",
      `state=${j06Fields["sidecar-job"]?.state ?? "缺失"} required=${j06Fields["sidecar-job"]?.required ?? "缺失"}`);
    check(`J-06 ${label} 降级后 sidecar 仍被正常回收`,
      j06Fields.shutdown?.reaped === "true" && j06Fields.shutdown?.sidecar_running === "false",
      `shutdown=${JSON.stringify(j06Fields.shutdown ?? null)}`);
  }

  console.log("");
  if (FAIL.length > 0) {
    console.log(`JOB OBJECT GATE: FAIL (${FAIL.length}) - ${FAIL.join("; ")}`);
    return 1;
  }
  console.log(
    `JOB OBJECT GATE: PASS (${passCount} checks)`
    + (NOT_VERIFIED.length ? ` [NOT VERIFIED: ${NOT_VERIFIED.length}]` : ""),
  );
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
