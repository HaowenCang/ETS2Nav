// E2E-LAN 局域网令牌 / 二维码 / 同源 WS（P4R Batch 3 §21）。
//
// 核心手法：从 `http://<本机 RFC1918 地址>:<port>/` 打开页面。本机连自己的私网
// 地址时内核选用的源地址就是该私网地址，因此 nav-server 看到的对端**确实**是
// PrivateLan 而非回环——远端来源矩阵无需第二台设备即可真实执行，也没有任何桩件
// 伪造来源。若浏览器把请求交给系统代理，对端会变成回环，用例会因此在下面
// 「无令牌必须 401」这类断言上失败（配置里已 `--no-proxy-server` 排除该路径）。
//
// E2E-LAN-03 是本批的关键回归测试：它锁定 PLAN-P3plus B5 的阻断项——页面只按
// 「同源」推导 WS 地址，与主机是否回环无关。原实现只在回环主机上推导，手机从
// LAN 地址打开时会去连 `ws://127.0.0.1:8123/ws`（手机上指向手机自己）。

import { expect, test } from "@playwright/test";
import { lanBootstrap, startServer } from "../harness.mjs";
import { session } from "../harness.mjs";
import { collectDiagnostics, assertCleanDiagnostics, waitConnected } from "./helpers.mjs";

/** 记录页面创建的每个 WebSocket 目标地址（含 query），用于断言令牌是否随连接携带。 */
async function recordWsUrls(page) {
  await page.addInitScript(() => {
    window.__wsUrls = [];
    const Native = window.WebSocket;
    window.WebSocket = function (...args) {
      window.__wsUrls.push(String(args[0]));
      return new Native(...args);
    };
    window.WebSocket.prototype = Native.prototype;
  });
}

/**
 * 记录 `#conn-status` 的**全部**文案变化。
 *
 * 断线提示只存在约 1 秒（退避基数是 1 s，随后就被「连接中…」和「已连接」覆盖），
 * 用轮询断言会随机漏掉它——即测试自身的不稳定，而非产品缺陷。这里改用
 * MutationObserver 累积每次变更，使「曾向用户报告过断线并排程重连」成为确定性事实。
 */
async function recordConnStatus(page) {
  await page.addInitScript(() => {
    window.__connTexts = [];
    document.addEventListener("DOMContentLoaded", () => {
      const el = document.getElementById("conn-status");
      if (!el) return;
      window.__connTexts.push(el.textContent);
      new MutationObserver(() => window.__connTexts.push(el.textContent))
        .observe(el, { childList: true, characterData: true, subtree: true });
    });
  });
}

const wsUrlCount = (page) => page.evaluate(() => window.__wsUrls.length);
const connTexts = (page) => page.evaluate(() => window.__connTexts);
const RECONNECT_NOTICE = /^已断开（\d+s 后重连）$/;

/** 打开指定绝对 URL 的正式页面并等待 MapLibre style 就绪。 */
async function gotoUrl(page, url) {
  await page.goto(url, { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => map.loaded() === true, null, { timeout: 20_000 });
}

/** 画布是否真的被绘制（QR 生成失败的空白画布必须被识别出来）。 */
async function canvasPainted(page) {
  return page.evaluate(() => {
    const c = document.getElementById("qr");
    if (!c || !c.width) return false;
    const ctx = c.getContext("2d");
    const { data } = ctx.getImageData(0, 0, c.width, c.height);
    let dark = 0;
    for (let i = 0; i < data.length; i += 4) {
      if (data[i] < 128) dark++;
    }
    return dark > 50; // 空白/纯色画布不会有成片的深色模块
  });
}

/** 本机候选私网地址；无候选时用例无法成立，必须显式失败而不是静默通过。 */
async function requireCandidates(port) {
  const boot = await lanBootstrap("127.0.0.1", port);
  expect(boot.enabled, "测试服务器必须以 --lan 启动").toBe(true);
  const addrs = boot.addresses ?? [];
  expect(
    addrs.length,
    "测试机没有任何 RFC1918 地址，无法从私网来源访问服务端——LAN 用例不可静默通过",
  ).toBeGreaterThan(0);
  return { boot, address: addrs[0].address };
}

// ─── E2E-LAN-01 二维码地址与令牌 ────────────────────────────────────────────

test("E2E-LAN-01 二维码使用真实 RFC1918 地址且 fragment 携带令牌", async ({ page }) => {
  const s = await session();
  const diag = collectDiagnostics(page);
  const { boot, address } = await requireCandidates(s.lanPort);

  await gotoUrl(page, `${s.lanOrigin}/`);
  await waitConnected(page);
  await page.click("#btn-settings");
  await expect(page.locator("#qr-box")).toBeVisible({ timeout: 15_000 });
  await expect(page.locator("#qr")).not.toHaveAttribute("data-qr-target", /^$/);

  const target = await page.getAttribute("#qr", "data-qr-target");
  expect(target, "二维码目标未写入（canvas 未被绘制）").not.toBeNull();
  const u = new URL(target);

  // 地址必须来自服务器提供的候选，且绝不能是回环
  expect(u.hostname).not.toBe("127.0.0.1");
  expect(u.hostname).not.toBe("localhost");
  expect(boot.addresses.map((a) => a.address)).toContain(u.hostname);
  expect(u.hostname).toBe(address);
  expect(
    /^(10\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.)/.test(u.hostname),
    `二维码地址不是 RFC1918: ${u.hostname}`,
  ).toBe(true);
  expect(u.port).toBe(String(s.lanPort));

  // fragment 携带完整令牌，且与服务端当前令牌一致
  expect(u.hash).toMatch(/^#token=[0-9a-f]{64}$/);
  expect(u.hash.slice("#token=".length)).toBe(boot.token);

  await expect.poll(() => canvasPainted(page), { timeout: 10_000 }).toBe(true);

  // 可见文本只含地址——令牌不得进入可见 DOM（失败截图/trace 会把它带进 artifact）
  const shown = await page.locator("#qr-url").textContent();
  expect(shown).toContain(u.hostname);
  expect(shown).not.toContain(boot.token);

  assertCleanDiagnostics(diag, "E2E-LAN-01");
});

test("E2E-LAN-01b 多候选地址时提供选择而非静默挑选", async ({ page }) => {
  const s = await session();
  const { boot } = await requireCandidates(s.lanPort);

  await gotoUrl(page, `${s.lanOrigin}/`);
  await waitConnected(page);
  await page.click("#btn-settings");
  await expect(page.locator("#qr-box")).toBeVisible({ timeout: 15_000 });

  const options = await page.$$eval("#lan-address option", (os) =>
    os.map((o) => ({ value: o.value, label: o.textContent })));
  expect(options.length).toBe(boot.addresses.length);
  expect(options.map((o) => o.value)).toEqual(boot.addresses.map((a) => a.address));
  // 候选项必须带网卡名，用户才能在多网卡下做知情选择
  for (const o of options) {
    expect(o.label).toContain(o.value);
  }

  if (boot.addresses.length >= 2) {
    await expect(page.locator("#lan-pick")).toBeVisible();
    // 切换候选地址必须重绘二维码，而不是保持旧地址
    const other = boot.addresses[1].address;
    await page.selectOption("#lan-address", other);
    const target = await page.getAttribute("#qr", "data-qr-target");
    expect(new URL(target).hostname).toBe(other);
    await expect(page.locator("#qr-url")).toContainText(other);
  } else {
    await expect(page.locator("#lan-pick")).toBeHidden();
  }
});

// ─── E2E-LAN-02 / 03 fragment 引导与远端同源 WS ─────────────────────────────

test("E2E-LAN-02 fragment 令牌被读取、校验、写入 session 并从地址栏移除", async ({ page }) => {
  const s = await session();
  const { boot, address } = await requireCandidates(s.lanPort);
  await recordWsUrls(page);

  await gotoUrl(page, `http://${address}:${s.lanPort}/#token=${boot.token}`);

  // fragment 必须立即从地址栏消失
  expect(new URL(page.url()).hash).toBe("");
  // 令牌进入 sessionStorage 与内存
  expect(await page.evaluate(() => sessionStorage.getItem("ets2nav.sessionToken")))
    .toBe(boot.token);
  expect(await page.evaluate(() => sessionToken)).toBe(boot.token);

  await waitConnected(page);
});

test("E2E-LAN-02b 畸形 fragment 令牌被拒绝且不写入 session", async ({ page }) => {
  const s = await session();
  const { address } = await requireCandidates(s.lanPort);
  await recordWsUrls(page);

  await gotoUrl(page, `http://${address}:${s.lanPort}/#token=${"z".repeat(64)}`);
  expect(new URL(page.url()).hash, "畸形 fragment 同样必须被移除").toBe("");
  expect(await page.evaluate(() => sessionStorage.getItem("ets2nav.sessionToken"))).toBeNull();
  expect(await page.evaluate(() => sessionToken)).toBeNull();

  // 无有效令牌 → 服务端必须拒绝该私网来源的 WS
  await expect(page.locator("#conn-status")).not.toHaveClass(/on/, { timeout: 15_000 });
  const urls = await page.evaluate(() => window.__wsUrls);
  expect(urls.length).toBeGreaterThanOrEqual(1);
  expect(urls[0]).not.toContain("token=");
});

test("E2E-LAN-03 远端页面推导同源 LAN WS 并真实建立连接（B5 回归）", async ({ page }) => {
  const s = await session();
  const diag = collectDiagnostics(page);
  const { boot, address } = await requireCandidates(s.lanPort);
  await recordWsUrls(page);

  await gotoUrl(page, `http://${address}:${s.lanPort}/#token=${boot.token}`);

  const urls = await page.evaluate(() => window.__wsUrls);
  expect(urls.length).toBeGreaterThanOrEqual(1);
  const wsUrl = urls[0];

  // 关键：不是 127.0.0.1，而是页面自身的 LAN 主机 + 当前端口 + 令牌
  expect(wsUrl).not.toContain("127.0.0.1");
  expect(wsUrl).not.toContain("localhost");
  expect(wsUrl).toBe(`ws://${address}:${s.lanPort}/ws?token=${boot.token}`);

  // 且真的连上了并收到帧——只断言 URL 构造不足以排除「URL 对了但被 401 拒绝」
  await waitConnected(page);
  await expect.poll(
    async () => Number(await page.locator("#speed-val").textContent()),
    { timeout: 25_000 },
  ).toBeGreaterThan(0);

  // 页面在私网来源下，二维码区块应隐藏（手机端不是被扫码方）
  await page.click("#btn-settings");
  await expect(page.locator("#qr-box")).toBeHidden();

  assertCleanDiagnostics(diag, "E2E-LAN-03");
});

// ─── E2E-LAN-04 route 鉴权 ─────────────────────────────────────────────────

test("E2E-LAN-04 UI 设目的地携带 Bearer；清除令牌后请求被拒", async ({ page }) => {
  const s = await session();
  const { boot, address } = await requireCandidates(s.lanPort);

  await gotoUrl(page, `http://${address}:${s.lanPort}/#token=${boot.token}`);
  await waitConnected(page);

  // 1) 正常路径：真实 UI 点击 → 必须带 Authorization 且成功
  const p1 = page.waitForResponse(
    (r) => r.url().includes("/api/route") && r.request().method() === "POST",
    { timeout: 25_000 },
  );
  await page.fill("#dest-x", "-52925");
  await page.fill("#dest-z", "36510");
  await page.click("#btn-route");
  const r1 = await p1;
  expect(r1.status(), "持有令牌时路由必须成功").toBe(200);
  expect(
    r1.request().headers()["authorization"],
    "setRoute 必须经 apiFetch 携带 Bearer 令牌",
  ).toBe(`Bearer ${boot.token}`);

  // 2) 清除令牌：同一 UI 操作不得再成功
  await page.evaluate(() => {
    sessionToken = null;
    try { sessionStorage.removeItem("ets2nav.sessionToken"); } catch { /* 不可用 */ }
  });
  const p2 = page.waitForResponse(
    (r) => r.url().includes("/api/route") && r.request().method() === "POST",
    { timeout: 25_000 },
  );
  await page.click("#btn-route");
  const r2 = await p2;
  expect(r2.status(), "无令牌的私网来源请求必须 401").toBe(401);
  expect(r2.request().headers()["authorization"]).toBeUndefined();
});

// ─── E2E-LAN-05 重连保持令牌 ───────────────────────────────────────────────

test("E2E-LAN-05 断线自动重连继续携带当前令牌", async ({ page }) => {
  const s = await session();
  const srv = await startServer({ webRoot: s.webRoot, trace: s.trace, lan: true });
  try {
    const { boot, address } = await requireCandidates(srv.port);
    await recordWsUrls(page);
    await recordConnStatus(page);
    await gotoUrl(page, `http://${address}:${srv.port}/#token=${boot.token}`);
    await waitConnected(page);

    const before = await wsUrlCount(page);
    expect(before).toBe(1);

    // 主动断开当前 socket（不重启服务端，令牌仍然有效）
    await page.evaluate(() => {
      const sock = ws;
      sock.onopen = sock.onmessage = sock.onerror = null;
      const prevClose = sock.onclose;
      sock.onclose = (ev) => { if (prevClose) prevClose(ev); };
      sock.close();
    });

    // 等待**确实发生了新的连接构造**：这比断言只存在约 1 秒的状态文案可靠
    await expect.poll(() => wsUrlCount(page), { timeout: 30_000 })
      .toBeGreaterThan(before);
    await expect(page.locator("#conn-status")).toHaveClass(/on/, { timeout: 30_000 });

    // 断线曾被报告给用户并排程重连（经 MutationObserver 累积，不依赖轮询时机）
    const texts = await connTexts(page);
    expect(
      texts.some((t) => RECONNECT_NOTICE.test(t)),
      `状态栏未报告断线重连，实际序列: ${JSON.stringify(texts)}`,
    ).toBe(true);

    // 每一次连接（含重连）都必须带同一令牌与同一 LAN 主机
    const urls = await page.evaluate(() => window.__wsUrls);
    expect(urls.length).toBeGreaterThanOrEqual(2);
    for (const u of urls) {
      expect(u, "每次连接（含重连）都必须携带令牌").toContain(`?token=${boot.token}`);
      expect(u).toContain(`//${address}:${srv.port}/ws`);
    }
    // 重连后帧流仍在推进
    await expect.poll(
      async () => Number(await page.locator("#speed-val").textContent()),
      { timeout: 25_000 },
    ).toBeGreaterThan(0);
  } finally {
    await srv.stop();
  }
});

test("E2E-LAN-05b 服务端重启后旧令牌失效但不会被静默丢弃", async ({ page }) => {
  const s = await session();
  const srv = await startServer({ webRoot: s.webRoot, trace: s.trace, lan: true });
  let srv2 = null;
  try {
    const { boot, address } = await requireCandidates(srv.port);
    await recordWsUrls(page);
    await recordConnStatus(page);
    await gotoUrl(page, `http://${address}:${srv.port}/#token=${boot.token}`);
    await waitConnected(page);
    const before = await wsUrlCount(page);

    // 同端口重启：新进程生成新令牌（S6 已证明轮换），页面手里的旧令牌因此失效
    await srv.stop();
    srv2 = await startServer({
      webRoot: s.webRoot, trace: s.trace, lan: true, port: srv.port,
    });
    const after = await lanBootstrap("127.0.0.1", srv2.port);
    expect(after.token, "重启后必须生成新令牌").not.toBe(boot.token);

    // 必须尝试过重连（观察 socket 构造，而非瞬时文案）
    await expect.poll(() => wsUrlCount(page), { timeout: 40_000 })
      .toBeGreaterThan(before);

    const texts = await connTexts(page);
    expect(
      texts.some((t) => RECONNECT_NOTICE.test(t)),
      `状态栏未报告断线重连，实际序列: ${JSON.stringify(texts)}`,
    ).toBe(true);

    // 重连尝试仍在发送旧令牌（令牌没有被静默丢弃），但服务端必须拒绝——
    // 页面不得出现「已连接」状态。
    await page.waitForTimeout(8_000);
    const urls = await page.evaluate(() => window.__wsUrls);
    expect(
      urls.every((u) => u.includes("?token=")),
      "重连不得丢失令牌参数",
    ).toBe(true);
    await expect(page.locator("#conn-status")).not.toHaveClass(/on/);
  } finally {
    if (srv2) await srv2.stop();
    await srv.stop().catch(() => {});
  }
});
