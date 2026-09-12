// E2E-07 GLOSA（P4R Batch 2 §11，本轮核心 Gate）。
//
// 覆盖：区间建议、单值建议（min == max）、无建议、先有建议后信号消失（旧值不得残留）。
// 断言对象一律是 DOM 文本，不是 JSON。
//
// 数据来源分两类，均不使用任何字符串反推：
//   a) 真实链路——`--fake-signal` 剧本经真实 §38 计算产生的结构化 glosa 字段；
//   b) 确定性注入——经真实 WS 消息入口投递协议帧，覆盖回放无法稳定命中的边界
//      （非法/非有限数值、精确 50–60 区间）。
//
// ── P4R Batch 5.5 §3/§5：真实链路用例的三处确定性修复 ────────────────────────
//
// 1. `waitGlosa` 的 TOCTOU（helpers.mjs 内说明）——判定与取值改为同一次求值，
//    返回值保证是被判定为匹配的那个值，不再出现「等到区间、读回空串」。
// 2. 窗口由**帧**而不是秒确定。剧本每 40 帧换一个阶段，六阶段一周期；用例进入时
//    处于哪个阶段取决于浏览器启动与之前用例的耗时，用墙上时间描述窗口等于把
//    「覆盖全部阶段需要多少帧」换成对 runner 速度的猜测。
// 3. 前置条件由测试自己维持。§38 只在会话 `Navigating` 分支内计算，而回放循环每轮
//    重建 session（目的地随之丢失）、车辆驶到 trace 终点附近会进入 Arrived——两者都
//    取决于用例开始时车辆处于 cycle 的哪一段，是同一类隐含 wall-time coupling。
//    因此采集循环在会话掉出 NAVIGATING 时经真实 UI 重新设目的地，而不是等更久。

import { expect, test } from "@playwright/test";
import {
  collectNavigatingEvidence, collectDiagnostics, deliverFrame, detachStream,
  dumpUiState, GLOSA_DEST, gotoApp, installFrameSampler, parseGlosa, readFrameSamples,
  setDestinationViaUi, startFrameSampling, vehicleFrame, waitConnected, waitGlosa,
} from "./helpers.mjs";
import { session } from "../harness.mjs";

/**
 * GLOSA 证据窗口的墙上时间兜底。
 *
 * 窗口的**计量单位是导航中的帧数**（一个完整剧本周期 240 帧 ≈ 12 s 标称），
 * 该值只负责在帧流已死时把「永远等下去」变成带诊断的失败，不参与相位对齐，
 * 也不为覆盖会话重建预留余量——那由前置条件维护解决。90 s ≈ 标称的 7.5 倍。
 */
const GLOSA_DEADLINE_MS = 90_000;

const RANGE_RE = /^建议 \d+–\d+ km\/h$/;

/** 打开信号服务器页面、装上采样器并建立导航会话（真实链路用例的公共前置）。 */
async function openNavigating(page, origin) {
  await installFrameSampler(page);
  await gotoApp(page, origin);
  await waitConnected(page);
  const installed = await startFrameSampling(page);
  expect(installed, `帧采样器未装上（dispatchMessage 不可用）：${installed}`).toBe(true);
  // GLOSA 只在 Navigating 状态下计算（session.rs 的 §36/§37/§38 分支），
  // 因此必须先经真实 UI 设目的地，否则状态恒为 Idle、glosa 恒为 null。
  await setDestinationViaUi(page, GLOSA_DEST);
  await expect(page.locator("#state-chip")).toHaveText("NAVIGATING", { timeout: 25_000 });
}

/** 采集导航中的帧证据并断言窗口完成；失败信息带完整诊断。 */
async function evidenceOrFail(page, needStates, minCounts = {}) {
  const ev = await collectNavigatingEvidence(page, needStates, GLOSA_DEADLINE_MS, minCounts);
  const samples = await readFrameSamples(page);
  const diag = JSON.stringify(await dumpUiState(page));
  expect(
    ev.reason,
    `导航中的帧证据窗口未覆盖 ${JSON.stringify(needStates)}`
    + `${Object.keys(minCounts).length ? `（最少帧数 ${JSON.stringify(minCounts)}）` : ""}：`
    + `${JSON.stringify(ev)}；重设目的地次数=${ev.reposts}。诊断：${diag}`,
  ).toBe("states-observed");
  return samples.filter((x) => x.session === "NAVIGATING");
}

test("E2E-07a 真实 §38 计算结果渲染区间 / 无建议", async ({ page }) => {
  test.setTimeout(150_000);
  const s = await session();
  await openNavigating(page, s.signalOrigin);

  const nav = await evidenceOrFail(page, ["red", "yellow", ""]);
  const reds = nav.filter((x) => x.state === "red");
  const yellows = nav.filter((x) => x.state === "yellow");
  const none = nav.filter((x) => x.state === "");

  // 红灯阶段：结构化 glosa 进入 DOM，文本为「建议 A–B km/h」且 A < B、A ≥ 0。
  // 这些区间与车速无关：三个红灯阶段的可达上限分别被 d/t1、限速 cap 决定，
  // 而可达加速度上限 sqrt(v²+2ad) 在所有阶段都大于它们（推导见报告 §5）。
  const rangeTexts = new Set();
  for (const x of reds) {
    expect(x.glosa, `红灯阶段 #glosa 必须是区间形态，实际 ${JSON.stringify(x.glosa)}`)
      .toMatch(RANGE_RE);
    const r = parseGlosa(x.glosa);
    expect(r, `区间文本非法: ${x.glosa}`).not.toBeNull();
    expect(r.min).toBeLessThan(r.max);
    expect(r.min).toBeGreaterThanOrEqual(0);
    // 服务端剧本中红灯区间只有 15–40 / 15–65 / 5–15 三种
    expect(["15–40", "15–65", "5–15"]).toContain(`${r.min}–${r.max}`);
    rangeTexts.add(`${r.min}–${r.max}`);
  }
  expect(rangeTexts.size, "红灯阶段必须至少产生一种区间").toBeGreaterThan(0);

  // 黄灯阶段：信号仍在，但 GLOSA 必须为空
  expect(yellows.length, "窗口内必须出现黄灯阶段").toBeGreaterThan(0);
  for (const x of yellows) {
    expect(x.hidden, `黄灯阶段卡片必须可见：${JSON.stringify(x)}`).toBe(false);
    expect(x.glosa, `黄灯阶段 GLOSA 必须为空：${JSON.stringify(x)}`).toBe("");
  }

  // 无信号阶段：卡片隐藏，GLOSA 仍为空
  expect(none.length, "窗口内必须出现无信号阶段").toBeGreaterThan(0);
  for (const x of none) {
    expect(x.hidden, `无信号阶段卡片必须隐藏：${JSON.stringify(x)}`).toBe(true);
    expect(x.glosa, `无信号阶段 GLOSA 必须为空：${JSON.stringify(x)}`).toBe("");
  }
});

test("E2E-07e 已知缺口：绿灯窗口不产生 GLOSA（§38 仅红灯接入）", async ({ page }) => {
  // 本用例固定一个**既有产品缺口**，不是通过标准：
  //   `glosa_advice` 本身支持绿灯窗口（nav-router 单测已证），但 session 中 §38 的
  //   调用嵌套在 `state == Red` 分支内（P3 关门报告记为「§38 GLOSA 仅红灯接入
  //   （绿灯窗口未接入）」），故绿灯阶段 UI 永远拿不到建议。
  // 本批不修改该门槛（属 §38 语义扩展，超出范围），改为显式登记：
  // 一旦绿灯接入被实现，本用例会失败并提示同步更新 E2E 与报告。
  //
  // P4R Batch 5.5 §5：原实现「等到绿灯后再采样 1.5 s」有一个真实竞态——绿灯只驻留
  // 2 s，若等待在绿灯末尾解出，采样窗内可能一个绿灯样本都没有，断言
  // `duringGreen.length > 0` 直接失败。现改为在同一份帧证据里取绿灯样本，并要求
  // 样本数达到半个阶段的量级（20 帧），使「绿灯期间 GLOSA 为空」不是空断言。
  // 同时补一条**正向对照**：窗口内必须出现带区间的红灯样本——否则 GLOSA 整体失效时
  // 「绿灯为空」会假通过。
  test.setTimeout(150_000);
  const s = await session();
  await openNavigating(page, s.signalOrigin);

  const nav = await evidenceOrFail(page, ["red", "green"], { green: 20 });
  const greens = nav.filter((x) => x.state === "green");
  const reds = nav.filter((x) => x.state === "red");

  expect(
    greens.length,
    `窗口内绿灯样本必须达到半个阶段（20 帧）以上，实际 ${greens.length}`,
  ).toBeGreaterThanOrEqual(20);
  for (const x of greens) {
    expect(x.glosa, `绿灯期间 GLOSA 应为空（当前 §38 仅红灯接入）：${JSON.stringify(x)}`).toBe("");
  }
  // 正向对照：本会话的 GLOSA 链路确实在工作
  expect(
    reds.some((x) => RANGE_RE.test(x.glosa)),
    "正向对照：窗口内必须出现至少一帧带区间的红灯样本，否则「绿灯为空」无区分力",
  ).toBe(true);
});

test("E2E-07b 信号消失后旧 GLOSA 不残留", async ({ page }) => {
  test.setTimeout(150_000);
  const s = await session();
  await openNavigating(page, s.signalOrigin);

  // 先确保出现建议。`waitGlosa` 返回的**就是**被判定为匹配的那个值（同一次求值），
  // 因此这里的断言不依赖「读取时值还没变」。
  const shown = await waitGlosa(page, "range");
  expect(shown, `waitGlosa(range) 的返回值本身必须是区间形态，实际 ${JSON.stringify(shown)}`)
    .toMatch(RANGE_RE);
  expect(parseGlosa(shown)).not.toBeNull();

  const nav = await evidenceOrFail(page, ["yellow", ""]);

  // 有信号但不可行（黄灯）：卡片仍可见，GLOSA 必须清空
  const yellows = nav.filter((x) => x.state === "yellow");
  expect(yellows.length, "窗口内必须出现黄灯阶段").toBeGreaterThan(0);
  for (const x of yellows) {
    expect(x.hidden, `黄灯阶段卡片必须可见：${JSON.stringify(x)}`).toBe(false);
    expect(x.glosa, `黄灯阶段 GLOSA 必须清空：${JSON.stringify(x)}`).toBe("");
  }

  // 再到无信号：卡片隐藏，GLOSA 仍为空
  const none = nav.filter((x) => x.state === "");
  expect(none.length, "窗口内必须出现无信号阶段").toBeGreaterThan(0);
  for (const x of none) {
    expect(x.hidden, `无信号阶段卡片必须隐藏：${JSON.stringify(x)}`).toBe(true);
    expect(x.glosa, `无信号阶段 GLOSA 必须保持为空：${JSON.stringify(x)}`).toBe("");
  }
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
