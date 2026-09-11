// E2E-06 信号卡片（P4R Batch 2 §11）：Red / Yellow / Green / signal absent。
//
// 使用注入确定性灯态剧本的服务器（`--fake-signal`）。离线回放没有实机灯态
// （read_semaphores 在游戏未运行时返回 None），§36/§37/§38 否则不可达；剧本注入点
// 在 session 内部，因此这里看到的仍是真实 §38 计算与真实 JSON 契约的产物。
//
// 断言的是 DOM：卡片可见性、灯态文本、剩余时间文本。

import { expect, test } from "@playwright/test";
import { gotoApp, waitConnected } from "./helpers.mjs";
import { session } from "../harness.mjs";

/** 等待信号卡片进入指定灯态（大小写不敏感，匹配 app.js 去前缀后的文本）。 */
async function waitSignalState(page, state, timeout = 40_000) {
  await page.waitForFunction((want) => {
    const card = document.getElementById("signal-card");
    if (card.classList.contains("hidden")) return false;
    return document.getElementById("signal-state").textContent.trim().toLowerCase() === want;
  }, state.toLowerCase(), { timeout });
}

test("E2E-06 信号卡片覆盖 Red / Yellow / Green / 无信号", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.signalOrigin);
  await waitConnected(page);

  for (const state of ["Red", "Green", "Yellow"]) {
    await waitSignalState(page, state);
    await expect(page.locator("#signal-card")).toBeVisible();
    await expect(page.locator("#signal-state")).toHaveClass(new RegExp(`^${state.toLowerCase()}$`));
    // 剩余时间为 "剩余 N.Ns"
    await expect(page.locator("#signal-remaining")).toHaveText(/^剩余 \d+\.\ds$/);
  }

  // 无信号阶段：卡片隐藏，且信号文本被清空（不得残留上一帧灯态）
  await page.waitForFunction(() => {
    const card = document.getElementById("signal-card");
    return card.classList.contains("hidden")
      && document.getElementById("signal-state").textContent === ""
      && document.getElementById("signal-remaining").textContent === "";
  }, null, { timeout: 40_000 });
  await expect(page.locator("#signal-card")).toBeHidden();
});
