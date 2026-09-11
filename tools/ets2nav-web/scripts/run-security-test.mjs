#!/usr/bin/env node
// nav-server LAN 安全集成测试运行器（P4R Batch 3 §20）。
//
// 一条命令完成：生成确定性合成 trace → 启动默认模式 server 与 --lan server →
// 读取两次 --lan 启动的令牌（用于 S6 轮换）→ 运行 verify-lan-security.py →
// 停止全部服务器并透传退出码。
//
// 令牌轮换取「同端口先停后起」：第一进程的令牌作为 stale，第二进程的在跑服务上
// 作为 live，因此 S6 断言的是「旧令牌在新进程上失效」，而不是跨端口对比。
//
// 本脚本驱动的是**服务端安全边界**（绑定/来源分类/令牌/CORS），不是浏览器 UI；
// UI 侧的令牌与同源 WS 行为由 Playwright 套件承担（npm run test:e2e 的 LAN 组）。

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { DATASET, NAV_CLI, REPO_ROOT, bootstrap, disallowedLocalAddresses, freePort, startServer } from "../tests/harness.mjs";

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), "..", "verify-lan-security.py");
const WORK = join(tmpdir(), "ets2nav-security");
const TRACE = join(WORK, "security.navtrace");
const CFG = join(WORK, "lan-security-config.json");
const WEB_ROOT = join(REPO_ROOT, "tools", "ets2nav-web", "dist");

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
  console.log("[security] 生成合成 trace…");
  const code = await run(NAV_CLI, [
    "syntrace", "-58456,32832:-52925,36510", DATASET, TRACE,
  ], { cwd: join(REPO_ROOT, "nav-core") });
  if (code !== 0 || !existsSync(TRACE)) throw new Error(`syntrace 失败 (exit ${code})`);
  return TRACE;
}

const python = process.env.PYTHON ?? "python";

// --selftest 不需要服务器：直接验证判定机制本身
if (process.argv.includes("--selftest")) {
  process.exit(await run(python, [SCRIPT, "--selftest"]));
}

const trace = await ensureTrace();
await mkdir(WORK, { recursive: true });

const servers = [];
const start = async (opts) => {
  const s = await startServer({ webRoot: WEB_ROOT, trace, ...opts });
  servers.push(s);
  return s;
};

let code = 1;
try {
  // 默认模式：验证只绑回环，且**同样**生成令牌、同样要求令牌（Batch 3.5）
  const def = await start({});
  const defBoot = await bootstrap("127.0.0.1", def.port);
  const defaultToken = defBoot.token;

  // LAN 模式第一进程：取令牌 A 后停止
  const lanPort = await freePort();
  const first = await start({ lan: true, port: lanPort, log: () => {} });
  const bootA = await bootstrap("127.0.0.1", first.port);
  const tokenStale = bootA.token;
  await first.stop();
  servers.splice(servers.indexOf(first), 1);

  // LAN 模式第二进程：同端口重启，取令牌 B（在跑服务）
  const second = await start({ lan: true, port: lanPort, log: () => {} });
  const bootB = await bootstrap("127.0.0.1", second.port);
  const tokenLive = bootB.token;
  const candidates = (bootB.addresses ?? []).map((a) => a.address);

  console.log(`[security] 默认模式 :${def.port}  LAN 模式 :${second.port}`);
  console.log(`[security] 候选地址 ${candidates.length ? candidates.join(", ") : "(无)"}`);

  // 非 RFC1918 的本机地址（VPN/隧道/链路本地）：用于真实执行「不受允许来源被拒」
  const disallowed = disallowedLocalAddresses();
  console.log(`[security] 不受允许来源候选 ${disallowed.length ? disallowed.join(", ") : "(无)"}`);

  await writeFile(CFG, JSON.stringify({
    defaultPort: def.port,
    lanPort: second.port,
    defaultToken,
    tokenLive,
    tokenStale,
    candidates,
    disallowedCandidates: disallowed,
    datasetDirHint: DATASET.split(/[\\/]/).filter(Boolean).pop(),
  }, null, 2), "utf8");

  code = await run(python, [SCRIPT, CFG]);
  if (code !== 0) {
    console.error("[security] 服务端 stderr 末尾：");
    for (const s of servers) {
      console.error(s.stderr.split("\n").slice(-6).join("\n"));
    }
  }
} finally {
  for (const s of servers) {
    try { await s.stop(); } catch { /* 可能已退出 */ }
  }
}
process.exit(code);
