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

  // 暂停期判定：暂停起点是刚才，未达 8 s，车辆在行驶也不得恢复。
  //
  // P4R Batch 5.5：这里原先隐式假设"前面几步花掉的时间很短"。`followPausedAt` 是在
  // `dragstart` 时写入的，而此后还要派发 8 步 mousemove、mouseup、一次 expect.poll 与
  // 两次 evaluate；负载高时这些累计可以超过 `FOLLOW_RESUME_MS`（8000 ms），于是生产
  // 代码的自动恢复分支**合法**触发，本断言把它读成"未到时间就恢复了"。
  // hosted runner 上实测到过一次：该运行的 Web E2E 步骤耗时 448 s（同批典型约 280 s），
  // 同一套件 7.4m（典型 4.6m）。
  //
  // 修法是把"未到时间"变成由测试确定的量：显式把暂停起点重置为当下（与下面折叠等待
  // 用的是同一手法），并在同一次求值里同时取回 `following` 与已过时间，使断言能与
  // "机器太慢以致本判据无意义"区分开。生产阈值与恢复条件均未改动。
  //
  // 计时基准必须是**测试自己记下的时刻**，不能读 `followPausedAt`：生产代码的
  // `resumeFollow()` 会把它清成 0，于是"产品提前恢复"会被算成
  // `Date.now() - 0`（一个天文数字）而被误判为环境问题——本用例的 mutation 验证
  // （把阈值改成 500 ms）第一次就命中了这个误判，故此处以 armedAt 为准。
  const armedAt = await page.evaluate(() => {
    followPausedAt = Date.now();
    return followPausedAt;
  });
  await page.waitForTimeout(1200);
  const mid = await page.evaluate((t0) => ({ following, elapsed: Date.now() - t0 }), armedAt);
  expect(
    mid.elapsed,
    `暂停起点被重置后已过 ${mid.elapsed} ms；若该值接近或超过 4000 ms，说明本机慢到使`
    + `本判据不再有区分力，应作为环境问题报告而不是产品提前恢复`,
  ).toBeLessThan(4000);
  expect(mid.following, "未到恢复时间不得自动恢复").toBe(false);

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

// ─── E2E-09d / 09e：P4R Batch 4 根因回归 ──────────────────────────────────
//
// 根因（实测，见 Batch 4 报告）：onSnapshot 每 50 ms 调一次 map.easeTo()，而
// MapLibre 的 easeTo 开头执行 `_stop(false, …)` → `HandlerManager.stop()` →
// 对**所有** handler 调 reset()，其中包括刚开始的拖拽（DragHandler._lastPoint 被
// 删除，之后的 mousemove 全被忽略）。于是 mousedown 与「位移首次超过 clickTolerance
// 的 mousemove」之间只要有**一次** easeTo 落下，这次拖动就永久失效：MapLibre 不发
// dragstart，反而在 mouseup 时发 click，挂在 dragstart 上的 pauseFollow 从不执行。
//
// 判别实验（每条件 16 次，只改变一个变量）：
//   跟随运行 + 按下后 8 ms 即移动        → dragstart 15/16
//   相机静止 + 同一段输入                → dragstart 16/16
//   跟随运行 + 按下后 ~60 ms 才移动      → dragstart  0/16   ← 真实用户所处区间
//   同上节奏，但手势期间不发 easeTo       → dragstart 16/16
// 每一次失败都对应「该窗口内确实夹了一次 easeTo」，无一次例外。
//
// E2E-09（Batch 2 原用例）的合成拖动在 mousedown 后约 8 ms 就移动，恰好落在容错
// 概率较高的一侧，因此只偶发失败（实测约 6–16%，负载下更高）。下面两项把根因钉住：
//   09d 用**真实用户的时间尺度**（按下后停留超过一个跟随周期再移动）——修复前 0/16，
//       修复后必须稳定通过；
//   09e 锁定刻意保留的语义边界：普通点击（无位移）**不**暂停跟随。

/** 记录 MapLibre 是否真的识别出了这次手势（库事件，独立于应用状态）。 */
async function recordDragEvents(page) {
  await page.addInitScript(() => {
    window.__dragstarts = 0;
    const iv = setInterval(() => {
      if (typeof map === "undefined" || !map || typeof map.on !== "function") return;
      clearInterval(iv);
      map.on("dragstart", () => { window.__dragstarts++; });
    }, 5);
  });
}

test("E2E-09d 真实时间尺度的拖动必须暂停跟随（修复前 0/16）", async ({ page }) => {
  const s = await session();
  await recordDragEvents(page);
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  expect((await followState(page)).following).toBe(true);

  // 前置：跟随动画确实在跑。没有它就没有竞争对象，本用例也就失去意义。
  await expect
    .poll(async () => page.evaluate(() => map.isEasing() || map.isMoving()), { timeout: 15_000 })
    .toBe(true);

  const box = await page.locator("#map").boundingBox();
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  // 手势形状，不是同步等待：真实按下后到首次移动之间必然有一段停留。实测该段在
  // 50–70 ms（见判别实验 C 条件：min 53 / max 74 / 均值 60 ms），超过一个跟随周期
  // （50 ms），正是修复前 100% 失效的区间。按住 60 ms 后再移动即复现该形状。
  await page.waitForTimeout(60);
  for (let i = 1; i <= 8; i++) {
    await page.mouse.move(cx + (60 * i) / 8, cy + (40 * i) / 8, { steps: 1 });
    // 每一步之间等一个真实渲染帧：使手势像真人一样跨越多帧，而不是同批派发
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  }
  await page.mouse.up();

  // 1) §57 契约：拖动后必须暂停
  await expect.poll(async () => (await followState(page)).following, { timeout: 15_000 }).toBe(false);
  const paused = await followState(page);
  expect(paused.hintHidden, "暂停后必须显示提示条").toBe(false);
  expect(paused.pausedAt).toBeGreaterThan(0);

  // 2) 手势必须真的被地图识别：只断言标志位会把「跟随暂停了但地图仍然拖不动」
  //    误判为通过——修复前正是「标志位没变 + 手势被丢弃」同时发生。
  expect(
    await page.evaluate(() => window.__dragstarts),
    "MapLibre 未识别出这次拖动（手势被跟随动画丢弃）",
  ).toBeGreaterThanOrEqual(1);

  // 3) 相机必须真的停下：暂停后不得再有跟随缓动
  await expect.poll(async () => page.evaluate(() => map.isEasing()), { timeout: 5_000 }).toBe(false);

  // 4) 用户仍可合法恢复跟随（既有 UX：◎ 按钮）
  await page.click("#btn-follow");
  expect((await followState(page)).following).toBe(true);
  await expect(page.locator("#follow-hint")).toBeHidden();
});

test("E2E-09e 普通点击（无位移）不得暂停跟随", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  expect((await followState(page)).following).toBe(true);

  const box = await page.locator("#map").boundingBox();
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  // 地图上没有点击交互（app.js 未注册 map click 处理器），因此一次不带位移的点击
  // 不该中断跟随。修复把暂停判据接到原始指针输入上，本用例锁定这一语义边界：
  // 位移阈值为 MapLibre 默认 clickTolerance（3 px），低于阈值的抖动不算拖动。
  const before = await page.evaluate(() => {
    const c = map.getCenter();
    return [c.lng, c.lat];
  });
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.mouse.up();

  expect((await followState(page)).following, "点击不得暂停跟随").toBe(true);
  // 真实条件（非固定等待）：点击之后相机必须继续随车辆移动——先变化的中心坐标
  // 本身就证明跟随循环仍在驱动地图。
  await expect.poll(async () => {
    const c = await page.evaluate(() => {
      const cc = map.getCenter();
      return [cc.lng, cc.lat];
    });
    return Math.hypot(c[0] - before[0], c[1] - before[1]) > 1e-6;
  }, { timeout: 15_000 }).toBe(true);
  expect((await followState(page)).hintHidden, "未暂停则不得显示提示条").toBe(true);
});
