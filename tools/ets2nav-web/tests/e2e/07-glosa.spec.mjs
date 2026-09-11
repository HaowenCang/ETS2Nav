// E2E-07 GLOSA（P4R Batch 2 §11，本轮核心 Gate）。
//
// 覆盖：区间建议、单值建议（min == max）、无建议、先有建议后信号消失（旧值不得残留）。
// 断言对象一律是 DOM 文本，不是 JSON。
//
// 数据来源分两类，均不使用任何字符串反推：
//   a) 真实链路——`--fake-signal` 剧本经真实 §38 计算产生的结构化 glosa 字段；
//   b) 确定性注入——经真实 WS 消息入口投递协议帧，覆盖回放无法稳定命中的边界
//      （非法/非有限数值、精确 50–60 区间）。

import { expect, test } from "@playwright/test";
import {
  collectDiagnostics, deliverFrame, detachStream, gotoApp, parseGlosa,
  setDestinationViaUi, vehicleFrame, waitConnected, waitGlosa,
} from "./helpers.mjs";
import { session } from "../harness.mjs";

test("E2E-07a 真实 §38 计算结果渲染区间 / 无建议", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.signalOrigin);
  await waitConnected(page);
  // GLOSA 只在 Navigating 状态下计算（session.rs 的 §36/§37/§38 分支），
  // 因此必须先经真实 UI 设目的地，否则状态恒为 Idle、glosa 恒为 null。
  await setDestinationViaUi(page, { x: -52925, z: 36510 });
  await expect(page.locator("#state-chip")).toHaveText("NAVIGATING", { timeout: 25_000 });

  // 红灯阶段：结构化 glosa 进入 DOM，文本为「建议 A–B km/h」且 A < B、A ≥ 0
  const rangeText = await waitGlosa(page, "range");
  const range = parseGlosa(rangeText);
  expect(range, `区间文本非法: ${rangeText}`).not.toBeNull();
  expect(range.min).toBeLessThan(range.max);
  expect(range.min).toBeGreaterThanOrEqual(0);
  // 服务端剧本中红灯区间只有 15–40 / 15–65 / 5–15 三种
  expect(["15–40", "15–65", "5–15"]).toContain(`${range.min}–${range.max}`);

  // 黄灯阶段：信号仍在，但 GLOSA 必须为空
  await page.waitForFunction(() => {
    const st = document.getElementById("signal-state").textContent.trim().toLowerCase();
    const card = document.getElementById("signal-card");
    return !card.classList.contains("hidden") && st === "yellow"
      && document.getElementById("glosa").textContent === "";
  }, null, { timeout: 60_000 });

  // 无信号阶段：卡片隐藏，GLOSA 仍为空
  const emptyText = await waitGlosa(page, "empty");
  expect(emptyText).toBe("");
});

test("E2E-07e 已知缺口：绿灯窗口不产生 GLOSA（§38 仅红灯接入）", async ({ page }) => {
  // 本用例固定一个**既有产品缺口**，不是通过标准：
  //   `glosa_advice` 本身支持绿灯窗口（nav-router 单测已证），但 session 中 §38 的
  //   调用嵌套在 `state == Red` 分支内（P3 关门报告记为「§38 GLOSA 仅红灯接入
  //   （绿灯窗口未接入）」），故绿灯阶段 UI 永远拿不到建议。
  // 本批不修改该门槛（属 §38 语义扩展，超出 Batch 2 范围），改为显式登记：
  // 一旦绿灯接入被实现，本用例会失败并提示同步更新 E2E 与报告。
  const s = await session();
  await gotoApp(page, s.signalOrigin);
  await waitConnected(page);
  await setDestinationViaUi(page, { x: -52925, z: 36510 });
  await expect(page.locator("#state-chip")).toHaveText("NAVIGATING", { timeout: 25_000 });

  // 找到绿灯驻留期，确认整段内 GLOSA 始终为空
  await page.waitForFunction(
    () => document.getElementById("signal-state").textContent.trim().toLowerCase() === "green",
    null, { timeout: 60_000 });
  const greenSamples = await page.evaluate(async () => {
    const out = [];
    const t0 = Date.now();
    while (Date.now() - t0 < 1500) {
      out.push({
        st: document.getElementById("signal-state").textContent.trim().toLowerCase(),
        glosa: document.getElementById("glosa").textContent,
      });
      await new Promise((r) => setTimeout(r, 100));
    }
    return out;
  });
  const duringGreen = greenSamples.filter((x) => x.st === "green");
  expect(duringGreen.length, "必须在绿灯驻留期内采样").toBeGreaterThan(0);
  for (const x of duringGreen) {
    expect(x.glosa, "绿灯期间 GLOSA 应为空（当前 §38 仅红灯接入）").toBe("");
  }
});

test("E2E-07b 信号消失后旧 GLOSA 不残留", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.signalOrigin);
  await waitConnected(page);
  await setDestinationViaUi(page, { x: -52925, z: 36510 });
  await expect(page.locator("#state-chip")).toHaveText("NAVIGATING", { timeout: 25_000 });

  // 先确保出现建议
  const shown = await waitGlosa(page, "range");
  expect(parseGlosa(shown)).not.toBeNull();

  // 等到「有信号但不可行」（黄灯）——此时信号卡片仍可见，GLOSA 必须清空
  await page.waitForFunction(() => {
    const card = document.getElementById("signal-card");
    const st = document.getElementById("signal-state").textContent.trim().toLowerCase();
    return !card.classList.contains("hidden")
      && st === "yellow"
      && document.getElementById("glosa").textContent === "";
  }, null, { timeout: 60_000 });
  await expect(page.locator("#glosa")).toHaveText("");
  await expect(page.locator("#signal-card")).toBeVisible();

  // 再到无信号：卡片隐藏，GLOSA 仍为空
  await page.waitForFunction(() => {
    const card = document.getElementById("signal-card");
    return card.classList.contains("hidden") && document.getElementById("glosa").textContent === "";
  }, null, { timeout: 60_000 });
  await expect(page.locator("#glosa")).toHaveText("");
});

test("E2E-07c 精确契约 50–60 与单值 50（确定性注入）", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  await detachStream(page);

  const withSignal = {
    state: "Red", remaining_s: 8.0, confidence: "Verified",
  };
  await deliverFrame(page, vehicleFrame({ upcoming_signal: withSignal, glosa: { min_kmh: 50, max_kmh: 60 } }));
  await expect(page.locator("#glosa")).toHaveText("建议 50–60 km/h");

  await deliverFrame(page, vehicleFrame({ upcoming_signal: withSignal, glosa: { min_kmh: 50, max_kmh: 50 } }));
  await expect(page.locator("#glosa")).toHaveText("建议 50 km/h");

  await deliverFrame(page, vehicleFrame({ upcoming_signal: withSignal, glosa: null }));
  await expect(page.locator("#glosa")).toHaveText("");

  // 信号消失：state / remaining / glosa 三者必须同时清空
  await deliverFrame(page, vehicleFrame({ upcoming_signal: null, glosa: null }));
  await expect(page.locator("#signal-card")).toBeHidden();
  await expect(page.locator("#signal-state")).toHaveText("");
  await expect(page.locator("#signal-remaining")).toHaveText("");
  await expect(page.locator("#glosa")).toHaveText("");
});

test("E2E-07d 非法数值被拒绝，不由 UI 修正", async ({ page }) => {
  const s = await session();
  const diag = collectDiagnostics(page);
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  await detachStream(page);

  const withSignal = { state: "Green", remaining_s: 8.0, confidence: "Verified" };
  const bad = [
    { name: "负数下限", glosa: { min_kmh: -10, max_kmh: 50 } },
    { name: "上限小于下限", glosa: { min_kmh: 60, max_kmh: 50 } },
    { name: "非数值", glosa: { min_kmh: "50", max_kmh: 60 } },
    { name: "缺字段", glosa: { min_kmh: 50 } },
    { name: "非对象", glosa: 42 },
  ];
  for (const c of bad) {
    // 先放入一个合法值，确保「被拒绝」表现为清空而不是从未设置
    await deliverFrame(page, vehicleFrame({ upcoming_signal: withSignal, glosa: { min_kmh: 50, max_kmh: 60 } }));
    await expect(page.locator("#glosa")).toHaveText("建议 50–60 km/h");

    await deliverFrame(page, vehicleFrame({ upcoming_signal: withSignal, glosa: c.glosa }));
    await expect(page.locator("#glosa"), `应拒绝：${c.name}`).toHaveText("");
  }

  // 拒绝必须是可观测的（console.warn），而不是静默吞掉
  const warned = diag.warnings.filter((e) => e.text.includes("glosa"));
  expect(warned.length, "非法 glosa 必须产生诊断输出").toBeGreaterThan(0);

  // NaN / Infinity 在 JSON 中本就不合法；确认文本层面从不出现
  const text = await page.locator("#glosa").textContent();
  expect(text).not.toMatch(/NaN|Infinity/);
});
