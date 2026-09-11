// E2E-11 移动端 viewport 布局（P4R Batch 2 §11）。
//
// 这是浏览器布局门，不是 B5 真机性能测试：断言 390×844 视口下没有横向不可达区域、
// 关键卡片可见、地图容器尺寸正常、打开设置与二维码后布局不被破坏。

import { expect, test } from "@playwright/test";
import { gotoApp, waitConnected } from "./helpers.mjs";
import { session } from "../harness.mjs";

const VIEWPORT = { width: 390, height: 844 };

/** 元素是否完全落在视口水平范围内。 */
async function withinViewportX(page, selector) {
  return page.evaluate((sel) => {
    const el = document.querySelector(sel);
    if (!el) return { found: false };
    const r = el.getBoundingClientRect();
    return {
      found: true,
      left: r.left,
      right: r.right,
      width: r.width,
      height: r.height,
      ok: r.left >= -1 && r.right <= window.innerWidth + 1 && r.width > 0,
    };
  }, selector);
}

test("E2E-11 移动端视口布局无横向溢出且卡片可见", async ({ page }) => {
  const s = await session();
  await page.setViewportSize(VIEWPORT);
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);

  // 无横向不可达区域
  const overflow = await page.evaluate(() => ({
    scrollWidth: document.scrollingElement.scrollWidth,
    innerWidth: window.innerWidth,
  }));
  expect(overflow.scrollWidth, "页面不得出现横向溢出").toBeLessThanOrEqual(overflow.innerWidth + 1);

  // 地图容器铺满视口
  const mapBox = await page.locator("#map").boundingBox();
  expect(mapBox.width).toBeCloseTo(VIEWPORT.width, 0);
  expect(mapBox.height).toBeCloseTo(VIEWPORT.height, 0);

  // 关键卡片可见且横向可达
  for (const sel of ["#speed-card", "#limit-card", "#state-chip", "#route-card", "#dest-bar"]) {
    await expect(page.locator(sel)).toBeVisible();
    const r = await withinViewportX(page, sel);
    expect(r.found, `${sel} 未找到`).toBe(true);
    expect(r.ok, `${sel} 超出视口: ${JSON.stringify(r)}`).toBe(true);
  }

  // 地图仍可与数据联动（不只是静态布局）
  await expect.poll(async () => Number(await page.locator("#speed-val").textContent()), { timeout: 20_000 })
    .toBeGreaterThan(0);
});

test("E2E-11b 设置面板与二维码不破坏移动端布局", async ({ page }) => {
  const s = await session();
  await page.setViewportSize(VIEWPORT);
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);

  await page.click("#btn-settings");
  await expect(page.locator("#settings-panel")).toBeVisible();

  const panel = await withinViewportX(page, "#settings-panel");
  expect(panel.ok, `设置面板超出视口: ${JSON.stringify(panel)}`).toBe(true);

  // 二维码只在 WS 连接成功后生成
  await expect(page.locator("#qr-box")).toBeVisible({ timeout: 15_000 });
  const qr = await withinViewportX(page, "#qr-box");
  expect(qr.ok, `二维码区域超出视口: ${JSON.stringify(qr)}`).toBe(true);
  const canvas = await page.evaluate(() => {
    const c = document.getElementById("qr");
    return { w: c.width, h: c.height };
  });
  expect(canvas.w).toBeGreaterThan(0);
  expect(canvas.h).toBeGreaterThan(0);

  // 打开面板后仍无横向溢出
  const overflow = await page.evaluate(() => ({
    scrollWidth: document.scrollingElement.scrollWidth,
    innerWidth: window.innerWidth,
  }));
  expect(overflow.scrollWidth).toBeLessThanOrEqual(overflow.innerWidth + 1);

  // 关闭面板后关键卡片仍在
  await page.click("#btn-settings");
  await expect(page.locator("#settings-panel")).toBeHidden();
  await expect(page.locator("#speed-card")).toBeVisible();
});
