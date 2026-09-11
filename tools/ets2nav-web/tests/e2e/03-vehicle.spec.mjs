// E2E-03 车辆快照（P4R Batch 2 §11）。
//
// 断言真实服务器帧流写入的 DOM：state / speed / 限速 / 剩余距离 / 目的地，
// 并确认 vehicle GeoJSON source 的坐标确实随时间变化。
//
// 会话在设目的地之前恒为 Idle（回放帧只推进遥测，不启动导航），故先经真实 UI
// 设目的地，再断言 NAVIGATING 及其派生字段。

import { expect, test } from "@playwright/test";
import { gotoApp, setDestinationViaUi, sourceCoordinates, waitConnected } from "./helpers.mjs";
import { session } from "../harness.mjs";

test("E2E-03 车辆快照驱动 DOM 与地图点", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);

  // vehicle source 坐标随时间变化（证明快照真的推到了地图层；与导航状态无关）
  const first = await sourceCoordinates(page, "vehicle");
  expect(Array.isArray(first), "vehicle source 坐标必须是数组").toBe(true);
  await expect
    .poll(async () => JSON.stringify(await sourceCoordinates(page, "vehicle")), { timeout: 20_000 })
    .not.toBe(JSON.stringify(first));

  // 经真实 UI 设目的地 → 进入 Navigating
  await setDestinationViaUi(page, { x: -52925, z: 36510 });
  await expect(page.locator("#state-chip")).toHaveText("NAVIGATING", { timeout: 25_000 });

  // 速度：数值 > 0（合成轨迹巡航段）
  await expect
    .poll(async () => Number(await page.locator("#speed-val").textContent()), { timeout: 20_000 })
    .toBeGreaterThan(0);

  // 限速：数值或占位符「–」（地图限速未知时不显示 0）
  const limit = (await page.locator("#limit-val").textContent())?.trim();
  expect(limit === "–" || /^\d+$/.test(limit), `限速文本非法: ${limit}`).toBe(true);

  // 剩余距离：到下一转向/终点的剩余里程，格式 "N.N km"
  await expect
    .poll(async () => (await page.locator("#route-remain").textContent())?.trim(), { timeout: 20_000 })
    .toMatch(/^\d+\.\d+ km$/);

  // 目的地文本已填充
  await expect(page.locator("#route-dest")).not.toHaveText("未设目的地");

  // 进度条宽度被写入（结构化进度被消费）
  const width = await page.evaluate(() => document.getElementById("route-progress-fill").style.width);
  expect(width).toMatch(/^\d+(\.\d+)?%$/);
});
