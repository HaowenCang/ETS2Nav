// E2E-09 跟随暂停 / 恢复（P4R Batch 2 §11）。
//
// 触发用真实浏览器事件（鼠标拖拽、滚轮），断言 following === false 与提示条可见。
//
// 自动恢复的时间控制：不真实 sleep 8 秒，也不使用 Playwright fake clock（它会一并
// 伪造 requestAnimationFrame，破坏 MapLibre 渲染）。改用「注入暂停起点时间」：
// 把 followPausedAt 设为距今超过 FOLLOW_RESUME_MS，随后等一帧真实 vehicle 数据到来，
// 由生产代码的恢复分支自行判定恢复。生产语义（阈值比较 + 车速条件）未被改动，
// 被折叠的只是墙钟等待。

import { expect, test } from "@playwright/test";
import { gotoApp, waitConnected } from "./helpers.mjs";
import { session } from "../harness.mjs";

async function followState(page) {
  return page.evaluate(() => ({
    following,
    pausedAt: followPausedAt,
    hintHidden: document.getElementById("follow-hint").classList.contains("hidden"),
  }));
}

test("E2E-09 拖拽暂停跟随、暂停期不恢复、到期后自动恢复", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);

  // 初始跟随，且跟随确实在工作（地图随车辆缓动）——这是拖拽测试的前置条件：
  // 若跟随尚未开始移动，拖拽事件与跟随状态机的交互并非被测场景。
  expect((await followState(page)).following).toBe(true);
  await expect
    .poll(async () => page.evaluate(() => map.isEasing() || map.isMoving()), { timeout: 15_000 })
    .toBe(true);

  // 真实鼠标拖拽（MapLibre 在位移超过阈值后触发 dragstart）
  const box = await page.locator("#map").boundingBox();
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.mouse.move(cx + 60, cy + 40, { steps: 8 });
  await page.mouse.up();

  await expect.poll(async () => (await followState(page)).following, { timeout: 15_000 }).toBe(false);
  const paused = await followState(page);
  expect(paused.hintHidden, "暂停后必须显示提示条").toBe(false);
  expect(paused.pausedAt).toBeGreaterThan(0);

  // 暂停期判定：暂停起点是刚才，未达 8 s，车辆在行驶也不得恢复
  await page.waitForTimeout(1200);
  expect((await followState(page)).following, "未到恢复时间不得自动恢复").toBe(false);

  // 折叠墙钟等待：把暂停起点前移 9 s，随后由生产代码在下一帧真实数据上自行恢复
  await page.evaluate(() => { followPausedAt = Date.now() - 9000; });
  await expect
    .poll(async () => (await followState(page)).following, { timeout: 20_000 })
    .toBe(true);
  await expect(page.locator("#follow-hint")).toBeHidden();
});

test("E2E-09b 滚轮同样触发暂停", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  expect((await followState(page)).following).toBe(true);

  const box = await page.locator("#map").boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, 240);

  await expect.poll(async () => (await followState(page)).following).toBe(false);
  await expect(page.locator("#follow-hint")).toBeVisible();
});

test("E2E-09c 手动点 ◎ 立即恢复跟随", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);

  const box = await page.locator("#map").boundingBox();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.wheel(0, 240);
  await expect.poll(async () => (await followState(page)).following).toBe(false);

  await page.click("#btn-follow");
  expect((await followState(page)).following).toBe(true);
  await expect(page.locator("#follow-hint")).toBeHidden();
});
