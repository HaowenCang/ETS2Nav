// Desktop 起源模型与 Tauri Origin 契约复核（P4R Batch 6A §5；取代 Batch 3.5 的 BLS-07）。
//
// ## 为什么判定方式变了
//
// Batch 3.5 的 BLS-07 在「`tauri.localhost` 静态前端 + 跨源访问回环 API」这一架构下
// 检查真实 Tauri 应用的请求 Origin：它在一个固定端口（8123）上放记录型转发器，
// 让应用经转发器访问另一个端口上的后端，从而读到应用实际发出的 `Origin` 头。
//
// Batch 6A §5 改为「sidecar 自己提供页面」：Desktop 启动随包 nav-core-cli，WebView
// 加载 `http://127.0.0.1:<运行时端口>/`。此时**页面与 API 同源**，跨源关系不再存在，
// 8123 这个固定端口与转发器也不再可用（Desktop 自己协商端口并直连它启动的进程）。
// 继续保留旧脚本只会得到一条永远失败的检查，因此改为验证新架构下的等价性质：
//
//   1. WebView 实际加载的页面**就是** sidecar 提供的那个源（host=127.0.0.1，
//      port=本次协商端口）——这是「同源」这一性质的可观测定义；
//   2. 页面不是 `tauri.localhost`，即产品不再依赖跨源模型；
//   3. 保留的 CORS 白名单仍然**按契约工作**：`http://tauri.localhost` 在
//      `/api/bootstrap` 上被接受并回带许可头，而非白名单 Origin 被拒。
//      本条与第 2 条并不矛盾：白名单是为兼容既有壳形态而保留的，本脚本同时证明
//      「它仍然有效」与「当前产品并不使用它」——后者若不说明，白名单会变成一条
//      无人使用、也无人验证的信任关系。
//
// 退出码：0 PASS；1 FAIL；3 NOT VERIFIED（无法执行，例如未构建 bundle 或无窗口环境）。
// 本脚本不打印令牌。

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { connect } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const args = process.argv.slice(2);
const argValue = (name, dflt = null) => {
  const i = args.indexOf(name);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : dflt;
};
const BUNDLE = argValue("--bundle", join(process.cwd(), "bundle"));

const FAIL = [];
let passCount = 0;
function check(name, ok, detail = "") {
  if (ok) { passCount += 1; console.log(`[PASS] ${name}${detail ? ` - ${detail}` : ""}`); }
  else { FAIL.push(name); console.log(`[FAIL] ${name}${detail ? ` - ${detail}` : ""}`); }
}
function notVerified(name, why) { console.log(`[NOT VERIFIED] ${name} - ${why}`); }

/** 极简原始 HTTP 请求：需要精确控制 `Origin` 头，因此不用 fetch（其头部处理不透明）。 */
function rawRequest(port, path, origin, extraHeaders = []) {
  return new Promise((ok, bad) => {
    const s = connect(port, "127.0.0.1");
    let buf = "";
    s.setTimeout(5000);
    s.on("connect", () => {
      const head = [
        `GET ${path} HTTP/1.1`,
        `Host: 127.0.0.1:${port}`,
        ...(origin ? [`Origin: ${origin}`] : []),
        ...extraHeaders,
        "Connection: close",
        "",
        "",
      ].join("\r\n");
      s.write(head);
    });
    s.on("data", (d) => { buf += d.toString("latin1"); });
    s.on("timeout", () => { s.destroy(); bad(new Error("请求超时")); });
    s.on("error", bad);
    s.on("close", () => {
      const status = Number(/^HTTP\/1\.1 (\d{3})/.exec(buf)?.[1] ?? 0);
      ok({ status, headers: buf.split("\r\n\r\n")[0], body: buf });
    });
  });
}

function parseLifecycle(stdout) {
  const rec = {};
  for (const line of stdout.split(/\r?\n/)) {
    const m = /^\[desktop\] ([a-z0-9_-]+)(?: (.*))?$/.exec(line.trim());
    if (!m) continue;
    const kv = {};
    for (const tok of (m[2] ?? "").split(/\s+/)) {
      const eq = tok.indexOf("=");
      if (eq > 0) kv[tok.slice(0, eq)] = tok.slice(eq + 1);
    }
    (rec[m[1]] ??= []).push(kv);
  }
  return rec;
}

async function waitFor(rec, key, pred, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    for (const kv of rec()[key] ?? []) if (pred(kv)) return kv;
    await new Promise((r) => setTimeout(r, 50));
  }
  return null;
}

async function main() {
  if (!existsSync(join(BUNDLE, "ets2nav-desktop.exe"))) {
    notVerified("Desktop 起源模型", `未找到 ${resolve(BUNDLE)}/ets2nav-desktop.exe（先运行 assemble-bundle.ps1）`);
    return 3;
  }
  const dataset = join(BUNDLE, "data", "europe-v5");
  const trace = join(tmpdir(), "ets2nav-desktop-origin.navtrace");
  if (!existsSync(trace)) {
    const r = await new Promise((ok) => {
      const p = spawn(join(BUNDLE, "nav-core-cli.exe"),
        ["syntrace", "-58456,32832:-52925,36510", dataset, trace],
        { stdio: "ignore", windowsHide: true });
      p.on("close", (code) => ok(code));
    });
    if (r !== 0) { notVerified("Desktop 起源模型", `syntrace 失败 exit=${r}`); return 3; }
  }

  console.log("=== Desktop 起源模型与 Tauri Origin 契约 ===");
  const proc = spawn(join(BUNDLE, "ets2nav-desktop.exe"), [`--replay=${trace}`],
    { cwd: BUNDLE, stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
  let stdout = "";
  proc.stdout.on("data", (d) => { stdout += d; });
  proc.stderr.on("data", () => {});
  const rec = () => parseLifecycle(stdout);

  try {
    const server = await waitFor(rec, "server", () => true, 150_000);
    const loaded = await waitFor(rec, "page_load", (kv) => kv.state === "Finished", 60_000);
    if (server === null) {
      notVerified("Desktop 起源模型", "Desktop 未在 150 s 内报告端口（无窗口环境？）");
      return 3;
    }
    const port = Number(server.port);
    check("Desktop 报告了 sidecar 协商端口", port > 0 && port !== 8123, `port=${port}`);
    if (loaded === null) {
      notVerified("WebView 页面来源", "未观察到 page_load=Finished（无交互式桌面？）");
      return 3;
    }
    const url = new URL(loaded.url);
    check("页面由 sidecar 自身提供（同源：host+port 与协商端口一致）",
      url.hostname === "127.0.0.1" && Number(url.port) === port,
      `page=${url.origin} sidecar=http://127.0.0.1:${port}`);
    check("页面不再使用 tauri.localhost（跨源模型已退出产品）",
      url.hostname !== "tauri.localhost", `host=${url.hostname}`);

    // 保留契约的正/负对照：白名单 Origin 被接受，非白名单被拒。
    const good = await rawRequest(port, "/api/bootstrap", "http://tauri.localhost");
    check("保留契约：白名单 Origin 在 /api/bootstrap 上被接受",
      good.status === 200 && /Access-Control-Allow-Origin: http:\/\/tauri\.localhost/i.test(good.headers),
      `status=${good.status} allow-origin=${/Access-Control-Allow-Origin: ([^\r\n]*)/i.exec(good.headers)?.[1] ?? "无"}`);
    const bad = await rawRequest(port, "/api/bootstrap", "http://evil.example");
    check("保留契约：非白名单 Origin 被拒且不回许可头",
      bad.status === 403 && !/Access-Control-Allow-Origin/i.test(bad.headers),
      `status=${bad.status}`);
    const wildcard = await rawRequest(port, "/api/bootstrap", "http://evil.example");
    check("保留契约：任何响应都不得出现通配符许可",
      !/Access-Control-Allow-Origin: \*/i.test(wildcard.headers));
  } finally {
    try { proc.kill("SIGKILL"); } catch { /* 已退出 */ }
  }

  console.log("");
  if (FAIL.length) {
    console.log(`DESKTOP ORIGIN: FAIL (${FAIL.length}) - ${FAIL.join("; ")}`);
    return 1;
  }
  console.log(`DESKTOP ORIGIN: PASS (${passCount} checks)`);
  return 0;
}

process.exit(await main());
