// E2E-07f `waitGlosa` 竞态回归（P4R Batch 5.5 §3）。
//
// 目的不是「再测一次 GLOSA 能显示」，而是把远端那次失败的**机制**固定成可判定的
// 实验：旧形态的 `waitGlosa` 是两次独立的浏览器往返——
//   ① `waitForFunction(predicate)`：谓词在帧 N 看到合法区间 → 解出；
//   ② `locator.textContent()`：再读一次 DOM，此时已是帧 N+1 的清空结果。
// 因此「判定成立」与「取到的值」可以不一致，`parseGlosa` 于是拿到 null。
//
// 构造方式刻意做成**确定性**而非靠时序碰运气。做法是在页面里把
// `document.getElementById` 包一层：谓词读取 `#glosa` 时在**当前宏任务末尾**
// （setTimeout 0）把文本清空，与 app.js 的真实行为同构（帧 N 写入区间、帧 N+1 清空）。
// 由此得到两个必然结果：
//   · 谓词这一次求值必然看到合法区间（同一次同步代码内的读取不会被翻转抢占）；
//   · 之后任何一次新的浏览器往返必然看到空串（翻转已经发生）。
//
// 之所以包 `document.getElementById` 而不是给元素加 `textContent` 访问器：实测
// `locator.textContent()` 不经由主世界的自有访问器（不同 realm 的元素包装），
// 用自有访问器计数会得到「只读了一次」的假象，实验便失去判别力。谓词侧调用的是
// `document.getElementById`，而定位器侧走选择器引擎，两者因此天然分离。
//
// 该用例同时是回归门：若有人把 `waitGlosa` 改回两次往返，新形态那一半会失败。

import { expect, test } from "@playwright/test";
import { parseGlosa, waitGlosa } from "./helpers.mjs";

const RANGE_RE = /^建议 \d+–\d+ km\/h$/;
const RANGE_TEXT = "建议 15–40 km/h";

/**
 * P4R Batch 5.5 之前的 `waitGlosa` 实现，原样保留作为**缺陷参照**。
 *
 * 保留理由：证明「判定与取值分离」这一形态确实可以返回与判定不一致的值——否则
 * 新实现只是在没有反例的情况下看起来更好，回归门也失去意义。
 */
async function legacyWaitGlosa(page, mode, timeout = 30_000) {
  await page.waitForFunction((m) => {
    const t = document.getElementById("glosa").textContent;
    if (m === "range") return /^建议 \d+–\d+ km\/h$/.test(t);
    if (m === "single") return /^建议 \d+ km\/h$/.test(t);
    if (m === "empty") return t === "";
    return true;
  }, mode, { timeout });
  return (await page.locator("#glosa").textContent()).trim();
}

/** 装上「谓词读取即安排清空」的钩子。 */
async function armHostileGlosa(page) {
  await page.evaluate((rangeText) => {
    const el = document.getElementById("glosa");
    el.textContent = rangeText;
    const state = { arms: 0, clears: 0, el };
    window.__glosa = state;
    const orig = document.getElementById.bind(document);
    document.getElementById = (id) => {
      const r = orig(id);
      if (id === "glosa") {
        state.arms += 1;
        // 帧 N+1 的清空：排在当前宏任务之后，不会抢占同一次求值内的读取
        setTimeout(() => { el.textContent = ""; state.clears += 1; }, 0);
      }
      return r;
    };
  }, RANGE_TEXT);
}

/** 复位为「合法区间 + 未清空」，供下一次实验使用。 */
async function resetHostileGlosa(page) {
  await page.evaluate((rangeText) => {
    // 必须经已保存的元素引用写入：复位本身若走被包裹的 getElementById，
    // 会立刻为自己安排一次清空，下一次实验便从空文本开始。
    window.__glosa.el.textContent = rangeText;
    window.__glosa.arms = 0;
    window.__glosa.clears = 0;
  }, RANGE_TEXT);
}

test("E2E-07f waitGlosa 竞态：旧形态可返回与判定不同的值，新形态不可能", async ({ page }) => {
  await page.setContent(
    "<!doctype html><meta charset=\"utf-8\"><title>glosa-race</title><div id=\"glosa\"></div>",
    { waitUntil: "domcontentloaded" },
  );
  await armHostileGlosa(page);

  // ── 缺陷参照：判定成立，取值不一致 ─────────────────────────────────────────
  await resetHostileGlosa(page);
  const legacy = await legacyWaitGlosa(page, "range", 10_000);
  const legacyState = await page.evaluate(() => ({ ...window.__glosa }));
  expect(
    legacyState.arms,
    "旧形态的谓词必须确实读过 #glosa（否则本次实验没有触发判别条件）",
  ).toBeGreaterThan(0);
  expect(
    legacy,
    `旧形态取到 ${JSON.stringify(legacy)}——与判定所见不一致，parseGlosa 因此返回 `
    + `${JSON.stringify(parseGlosa(legacy))}。这正是远端 E2E-07b 失败的机制。`,
  ).toBe("");
  expect(parseGlosa(legacy)).toBeNull();

  // ── 现形态：判定与取值同一次求值，返回值必然是被判定的那个值 ───────────────
  await resetHostileGlosa(page);
  const fixed = await waitGlosa(page, "range", 10_000);
  expect(
    fixed,
    `新形态返回值必须是区间本身，实际 ${JSON.stringify(fixed)}`,
  ).toMatch(RANGE_RE);
  expect(parseGlosa(fixed)).not.toBeNull();
  expect(parseGlosa(fixed).min).toBe(15);
  expect(parseGlosa(fixed).max).toBe(40);

  // ── 对照：清空之后，同一形态立刻表现为「无建议」────────────────────────────
  await page.evaluate(() => { document.getElementById("glosa").textContent = ""; });
  expect(await waitGlosa(page, "empty", 5_000)).toBe("");
});
