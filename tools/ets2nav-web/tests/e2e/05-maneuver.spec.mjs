// E2E-05 下一转向卡片（P4R Batch 2 §11）。
//
// 两条独立的证据：
//   真实链路——服务器的 maneuver 数据写入 #maneuver-type / #maneuver-dist；
//   确定性注入——经真实 WS 消息入口投递带 roundabout_exit 的帧，锁定环岛出口文案
//   （合成轨迹不一定经过环岛，故该分支无法靠回放稳定命中）。

import { expect, test } from "@playwright/test";
import { deliverFrame, detachStream, gotoApp, setDestinationViaUi, vehicleFrame, waitConnected } from "./helpers.mjs";
import { session } from "../harness.mjs";

test("E2E-05a 真实 maneuver 数据渲染转向类型与距离", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  await setDestinationViaUi(page, { x: -52925, z: 36510 });

  await expect(page.locator("#maneuver-card")).toBeVisible({ timeout: 25_000 });
  const type = (await page.locator("#maneuver-type").textContent())?.trim();
  expect(type, "转向类型必须非空").toBeTruthy();
  expect(type).not.toBe("–");
  // app.js 去掉 ManeuverType:: 前缀后直接显示
  expect(type).not.toMatch(/^ManeuverType::/);
  await expect(page.locator("#maneuver-dist")).toHaveText(/^\d+ m$/);
});

test("E2E-05b 环岛出口文案由结构化字段渲染（确定性注入）", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  await detachStream(page); // 停止 20 Hz 广播，避免注入被覆盖

  await deliverFrame(page, vehicleFrame({
    state: "navigating",
    next_maneuver: {
      type: "ManeuverType::Roundabout",
      distance_m: 123.456,
      bearing_deg: 45,
      roundabout_exit: 2,
    },
  }));
  await expect(page.locator("#maneuver-card")).toBeVisible();
  await expect(page.locator("#maneuver-type")).toHaveText("Roundabout");
  await expect(page.locator("#maneuver-dist")).toHaveText("123 m");
  await expect(page.locator("#maneuver-exit")).toHaveText("环岛出口 2");

  // 非环岛：出口文本必须清空（不得残留上一帧的出口编号）
  await deliverFrame(page, vehicleFrame({
    state: "navigating",
    next_maneuver: { type: "ManeuverType::TurnRight", distance_m: 80, bearing_deg: 90, roundabout_exit: null },
  }));
  await expect(page.locator("#maneuver-exit")).toHaveText("");

  // 非导航状态：卡片隐藏
  await deliverFrame(page, vehicleFrame({ state: "idle", next_maneuver: null }));
  await expect(page.locator("#maneuver-card")).toBeHidden();
});
