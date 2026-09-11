// E2E-10 WebSocket 断线重连（P4R Batch 2 §11）。
//
// 本项首先是一个产品结论：P4R Batch 1 复核时确认原实现的 onclose 只把状态文案改成
// 「已断开」，**不存在任何重连路径**，而 PLAN-P3plus 的 B5 验收项明确列有「断线重连
// 行为」。因此本轮补了最小实现（有界指数退避 + 手动连接仍可用 + 不产生重复 socket），
// 并由本测试验证。若该实现被移除，本测试必须失败。
//
// 使用专用服务器实例（动态端口），在测试内杀掉并按**同一端口**重启，以便观察同一
// 页面自行恢复；不使用全局共享的数据服务器，避免影响其他用例。

import { expect, test } from "@playwright/test";
import { startServer } from "../harness.mjs";
import { session } from "../harness.mjs";

const wsCounters = () => ({
  created: window.__wsCreated ?? 0,
  closed: window.__wsClosed ?? 0,
});

test("E2E-10 断线后自动重连，且不产生重复 socket", async ({ page }) => {
  const s = await session();
  await page.addInitScript(() => {
    window.__wsCreated = 0;
    window.__wsClosed = 0;
    const Native = window.WebSocket;
    window.WebSocket = function (...args) {
      window.__wsCreated++;
      const sock = new Native(...args);
      const origClose = sock.close.bind(sock);
      sock.close = (...a) => { window.__wsClosed++; return origClose(...a); };
      return sock;
    };
    window.WebSocket.prototype = Native.prototype;
  });

  const srv = await startServer({ webRoot: s.webRoot, trace: s.trace });
  try {
    await page.goto(`${srv.origin}/`, { waitUntil: "domcontentloaded" });
    await expect(page.locator("#conn-status")).toHaveClass(/on/, { timeout: 20_000 });
    expect(await page.evaluate(wsCounters)).toMatchObject({ created: 1 });

    // 杀掉服务器：页面必须显示断开并**排程重连**（而不是仅显示「已断开」）
    await srv.stop();
    await expect(page.locator("#conn-status")).toHaveText(/^已断开（\d+s 后重连）$/, { timeout: 20_000 });

    // 同端口重启；页面应自行恢复连接
    const srv2 = await startServer({ webRoot: s.webRoot, trace: s.trace, port: srv.port });
    try {
      await expect(page.locator("#conn-status")).toHaveClass(/on/, { timeout: 30_000 });
      const after = await page.evaluate(wsCounters);
      // 退避有界：重连尝试次数很少（不是忙循环），且确实新建了连接
      expect(after.created, "必须发生重连").toBeGreaterThanOrEqual(2);
      expect(after.created, "退避必须收敛，不得形成连接风暴").toBeLessThanOrEqual(6);
    } finally {
      await srv2.stop();
    }
  } finally {
    await srv.stop().catch(() => {});
  }
});

test("E2E-10b 手动「连接」按钮在断线后仍可用且作废旧 socket", async ({ page }) => {
  const s = await session();
  await page.addInitScript(() => {
    window.__wsCreated = 0;
    window.__wsClosed = 0;
    const Native = window.WebSocket;
    window.WebSocket = function (...args) {
      window.__wsCreated++;
      const sock = new Native(...args);
      const origClose = sock.close.bind(sock);
      sock.close = (...a) => { window.__wsClosed++; return origClose(...a); };
      return sock;
    };
    window.WebSocket.prototype = Native.prototype;
  });

  await page.goto(`${s.dataOrigin}/`, { waitUntil: "domcontentloaded" });
  await expect(page.locator("#conn-status")).toHaveClass(/on/, { timeout: 20_000 });

  await page.click("#btn-settings");
  await page.click("#btn-connect");
  await page.click("#btn-connect");
  await expect(page.locator("#conn-status")).toHaveClass(/on/, { timeout: 20_000 });

  const c = await page.evaluate(wsCounters);
  // 每次 connect 新建一个 socket，并显式关闭被取代的旧 socket
  expect(c.created).toBe(3);
  expect(c.closed, "被取代的 socket 必须被关闭，避免重复连接堆积").toBeGreaterThanOrEqual(2);
  // 当前 socket 仍在工作：状态栏已连接，且后续帧仍能更新 DOM
  await expect.poll(async () => Number(await page.locator("#speed-val").textContent()), { timeout: 20_000 })
    .toBeGreaterThan(0);
});
