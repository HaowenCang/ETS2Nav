// E2E-06 信号卡片（P4R Batch 2 §11）：Red / Yellow / Green / 无信号。
//
// 使用注入确定性灯态剧本的服务器（`--fake-signal`）。离线回放没有实机灯态
// （read_semaphores 在游戏未运行时返回 None），§36/§37/§38 否则不可达；剧本注入点
// 在 session 内部，因此这里看到的仍是真实 §38 计算与真实 JSON 契约的产物。
//
// 断言的是 DOM：卡片可见性、灯态文本、剩余时间文本。
//
// ── P4R Batch 5.5 §4：为什么不再按「Red → Green → Yellow」顺序等待 ──────────
//
// 剧本按**帧号**推进：`fake_signal_phase(f) = (f / 40) % 6`，六阶段依次为
//   0 红、1 红、2 黄、3 无信号、4 绿、5 红
// 而三个测试服务器在 globalSetup 建立后长期运行，因此某个用例进入时落在哪个阶段，
// 取决于浏览器启动耗时与之前全部用例的累计耗时。原实现按 Red→Green→Yellow 顺序
// 等待，等价于要求「恰好在这个相位序列上被看见」：
//   · 从任意相位出发，等到 Green(4) 后还要跨过 4→5→0→1→2 才能看到 Yellow(2)，
//     即最坏要走完 5 个阶段（200 帧 ≈ 10 s 标称，慢 runner 上按比例放大）；
//   · 每一步之后的 `toBeVisible()` / `toHaveClass()` / `toHaveText()` 又各是一次
//     独立的浏览器往返，而 Yellow 只驻留 2 s、Green 只驻留 2 s——判定与断言分处
//     两个时刻，正是远端那次 `page.waitForFunction: Test timeout of 60000ms exceeded`
//     的形态。
//
// 现改为**帧证据观察集**：包住真实 `dispatchMessage`，在每帧返回后同步采集该帧的
// DOM 契约，等「所需状态全部出现」或「帧预算用尽」为止，然后对采集到的**每一个**
// 样本逐条核对契约。完成条件由收到的帧数确定，与进入相位无关，也与 runner 快慢无关；
// 断言强度反而更高——原实现只检查「有一次相符」，现在检查「每一帧都相符」。

import { expect, test } from "@playwright/test";
import {
  dumpUiState, FAKE_SIGNAL_COVER_FRAMES, gotoApp, installFrameSampler,
  readFrameSamples, startFrameSampling, waitConnected, waitFrameEvidence,
} from "./helpers.mjs";
import { session } from "../harness.mjs";

/**
 * 证据窗口的**墙上时间兜底**（不承担相位对齐职责）。
 *
 * 240 帧 × 250 ms/帧：标称帧间隔由服务端自行节流为 50 ms（sim time 20 Hz），
 * 250 ms 是 5 倍退化余量。相位覆盖由帧预算保证，因此该值只在帧流已死
 * （服务端退出、WS 长时间断开）时把「永远等下去」变成一次带诊断的失败。
 */
const EVIDENCE_TIMEOUT_MS = 60_000;

test("E2E-06 信号卡片覆盖 Red / Yellow / Green / 无信号", async ({ page }) => {
  // 上限推导：页面建立（gotoApp 20 s + waitConnected 15 s 的既有上界）+ 证据窗口
  // 60 s + 契约核对余量。该值随证据窗口的推导变化，不是为压低失败率而放宽。
  test.setTimeout(120_000);

  const s = await session();
  await installFrameSampler(page);
  await gotoApp(page, s.signalOrigin);
  await waitConnected(page);
  const installed = await startFrameSampling(page);
  expect(installed, `帧采样器未装上（dispatchMessage 不可用）：${installed}`).toBe(true);

  let outcome;
  try {
    outcome = await waitFrameEvidence(page, ["red", "yellow", "green", ""], EVIDENCE_TIMEOUT_MS);
    expect(await outcome.jsonValue()).toBe("states-observed");
  } catch (e) {
    throw new Error(
      `未在 ${FAKE_SIGNAL_COVER_FRAMES} 帧证据窗口内观察到全部灯态。`
      + `原始错误：${e.message}\n诊断：${JSON.stringify(await dumpUiState(page))}`);
  }

  const samples = await readFrameSamples(page);
  const diag = JSON.stringify(await dumpUiState(page));
  const byState = new Map();
  for (const x of samples) {
    if (!byState.has(x.state)) byState.set(x.state, []);
    byState.get(x.state).push(x);
  }
  const observed = [...byState.keys()].sort();

  const label = (st) => (st === "" ? "无信号" : st);
  for (const st of ["red", "yellow", "green", ""]) {
    expect(
      observed,
      `证据窗口内未出现灯态「${label(st)}」（实际出现 ${JSON.stringify(observed)}，`
      + `样本 ${samples.length} 帧）。诊断：${diag}`,
    ).toContain(st);
  }

  // 契约核对：对每个样本逐条验证，而不是只验证「某一次相符」。
  // 采样发生在 dispatchMessage 返回后的同一同步块内，因此样本即该帧的渲染结果，
  // 不存在「等到之后状态又翻页」的窗口。
  for (const st of ["red", "yellow", "green"]) {
    for (const x of byState.get(st)) {
      expect(x.hidden, `${st} 阶段信号卡片必须可见。样本：${JSON.stringify(x)}`).toBe(false);
      expect(x.cls, `${st} 阶段 #signal-state 的 class 必须是灯态本身`).toBe(st);
      expect(
        x.remaining,
        `${st} 阶段剩余时间必须是「剩余 N.Ns」。样本：${JSON.stringify(x)}`,
      ).toMatch(/^剩余 \d+\.\ds$/);
    }
  }
  // 无信号阶段：卡片隐藏，且 state / remaining 文本必须被清空（不得残留上一帧灯态）
  for (const x of byState.get("")) {
    expect(x.hidden, `无信号阶段卡片必须隐藏。样本：${JSON.stringify(x)}`).toBe(true);
    expect(x.cls, `无信号阶段 class 必须清空。样本：${JSON.stringify(x)}`).toBe("");
    expect(x.remaining, `无信号阶段剩余时间必须清空。样本：${JSON.stringify(x)}`).toBe("");
  }

  // 采样器本身不得改变渲染结果：契约核对全部通过后，当前 DOM 仍必须是合法状态之一。
  const now = await page.evaluate(() => ({
    state: document.getElementById("signal-state").textContent.trim().toLowerCase(),
    cls: document.getElementById("signal-state").className,
    hidden: document.getElementById("signal-card").classList.contains("hidden"),
  }));
  expect(["red", "yellow", "green", ""], `当前灯态非法：${JSON.stringify(now)}`).toContain(now.state);
  if (now.state === "") expect(now.hidden, "无信号时卡片必须隐藏").toBe(true);
});
