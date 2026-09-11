#!/usr/bin/env node
// nav-server 协议集成测试运行器（P4R Batch 2 §7）。
//
// 一条命令完成：生成确定性合成 trace → 动态端口启动 nav-server（--replay）→
// 运行 verify-server-protocol.py → 停止服务器并透传退出码。
//
// 该脚本驱动的是**协议层**（HTTP 路由 / WS 帧 / 快照通道），不是浏览器 UI：
// UI / DOM / 地图断言由 Playwright 套件承担（npm run test:e2e）。

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { DATASET, NAV_CLI, REPO_ROOT, startServer } from "../tests/harness.mjs";

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "..", "verify-server-protocol.py");
const WORK = join(tmpdir(), "ets2nav-protocol");
const TRACE = join(WORK, "protocol.navtrace");

function run(cmd, args, opts = {}) {
  return new Promise((ok, bad) => {
    const p = spawn(cmd, args, { stdio: "inherit", ...opts });
    p.on("error", bad);
    p.on("close", (code) => ok(code ?? 1));
  });
}

async function ensureTrace() {
  if (existsSync(TRACE)) return TRACE;
  await mkdir(WORK, { recursive: true });
  console.log("[protocol] 生成合成 trace…");
  const code = await run(NAV_CLI, [
    "syntrace", "-58456,32832:-52925,36510", DATASET, TRACE,
  ], { cwd: join(REPO_ROOT, "nav-core") });
  if (code !== 0 || !existsSync(TRACE)) {
    throw new Error(`syntrace 失败 (exit ${code})`);
  }
  return TRACE;
}

const python = process.env.PYTHON ?? "python";

// --selftest 不需要服务器：直接运行 wraparound 判定的确定性用例
if (process.argv.includes("--selftest")) {
  process.exit(await run(python, [SCRIPT, "--selftest"]));
}

const trace = await ensureTrace();
const srv = await startServer({ webRoot: join(REPO_ROOT, "tools", "ets2nav-web", "dist"), trace });
let code = 1;
try {
  code = await run(python, [SCRIPT, String(srv.port)]);
} finally {
  await srv.stop();
  if (code !== 0) {
    console.error("[protocol] 服务器 stderr 末尾：");
    console.error(srv.stderr.split("\n").slice(-10).join("\n"));
  }
}
process.exit(code);
