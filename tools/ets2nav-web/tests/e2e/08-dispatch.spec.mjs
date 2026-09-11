// E2E-08 事件分发隔离（P4R Batch 2 §11）。
//
// 回归目标：历史上 map_state 帧被无条件交给 onSnapshot，因缺 state 字段抛错后被
// 静默吞掉，导致「服务端路线几何推送从未被渲染」；反向的错误（vehicle 帧进入
// onMapState）同样必须被排除。
//
// 断言：map_state 不进入 onSnapshot（状态 chip 不变）、vehicle 不进入 onMapState
// （路线折线不变），且 map_state 确实更新路线折线。

import { expect, test } from "@playwright/test";
import { deliverFrame, detachStream, gotoApp, sourceCoordinates, vehicleFrame, waitConnected } from "./helpers.mjs";
import { session } from "../harness.mjs";

test("E2E-08 vehicle 与 map_state 分发互不串道", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  await detachStream(page);

  // 起点：先投一条 vehicle 帧把状态与折线置为已知值
  await deliverFrame(page, vehicleFrame({ state: "navigating" }));
  await expect(page.locator("#state-chip")).toHaveText("NAVIGATING");
  await deliverFrame(page, {
    type: "map_state",
    distance_m: 0,
    polyline: [],
    destination: [0, 0],
  });
  await expect.poll(async () => (await sourceCoordinates(page, "route-line"))?.length ?? -1)
    .toBe(0);
  const before = await page.locator("#state-chip").textContent();

  // map_state 携带 5 点折线：折线更新，状态 chip 不得改变
  const polyline = [[0, 0], [0.001, 0], [0.001, 0.001], [0.002, 0.001], [0.002, 0.002]];
  await deliverFrame(page, { type: "map_state", distance_m: 250, polyline, destination: [0.002, 0.002] });
  await expect.poll(async () => (await sourceCoordinates(page, "route-line"))?.length ?? 0)
    .toBe(polyline.length);
  await expect(page.locator("#state-chip")).toHaveText(before);

  // vehicle 帧：状态 chip 变化，折线保持不变
  await deliverFrame(page, vehicleFrame({ state: "rerouting" }));
  await expect(page.locator("#state-chip")).toHaveText("REROUTING");
  const after = await sourceCoordinates(page, "route-line");
  expect(after.length).toBe(polyline.length);
});

test("E2E-08b 未知事件类型被计数并告警，不进入任一分发路径", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  await detachStream(page);

  await deliverFrame(page, vehicleFrame({ state: "navigating" }));
  await deliverFrame(page, { type: "map_state", distance_m: 0, polyline: [[0, 0], [1, 1]], destination: [1, 1] });
  const chip = await page.locator("#state-chip").textContent();
  const coords = await sourceCoordinates(page, "route-line");

  await deliverFrame(page, { type: "telemetry_bogus", state: "error" });
  const counter = await page.evaluate(() => unknownEventCount);
  expect(counter, "未知类型必须被计数").toBeGreaterThan(0);
  await expect(page.locator("#state-chip")).toHaveText(chip);
  expect((await sourceCoordinates(page, "route-line")).length).toBe(coords.length);
});

test("E2E-08c 服务端 map_state 真实到达并渲染路线（真实链路）", async ({ page }) => {
  const s = await session();
  const diag = await import("./helpers.mjs").then((m) => m.collectDiagnostics(page));
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);

  // 未设目的地时不应有路线几何
  const before = (await sourceCoordinates(page, "route-line"))?.length ?? 0;
  expect(before).toBeLessThanOrEqual(1);

  await page.fill("#dest-x", "-52925");
  await page.fill("#dest-z", "36510");
  await page.click("#btn-route");

  // 真实 map_state 帧到达并被 onMapState 消费
  await expect.poll(async () => (await sourceCoordinates(page, "route-line"))?.length ?? 0, { timeout: 25_000 })
    .toBeGreaterThan(1);

  // 历史缺陷的判别点：map_state 曾因进入 onSnapshot 抛错并被静默吞掉。现在它既
  // 不得改变状态 chip，也不得产生任何非白名单诊断（若仍抛错，onmessage 的
  // try/catch 会打 console.error，此处即失败）。
  await expect(page.locator("#state-chip")).toHaveText(/^[A-Z_]+$/);
  const bad = diag.entries.filter((e) => !e.url.includes("/vendor/fonts/"));
  expect(bad, `分发过程产生错误：${JSON.stringify(bad)}`).toHaveLength(0);
});
