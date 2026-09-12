// E2E 公共工具（P4R Batch 2）。
//
// 诊断采集采用精确白名单：唯一允许的 console error 是「本地 glyphs 探测 404」——
// 该降级在 P4R Batch 1 已确立（跳过 city 文字层，几何图层照常）。任何其他 404、
// 任何 pageerror、任何应用自身的 console.error 都必须使测试失败。
// 明确不做「忽略所有 404」或「忽略所有 console.error」。

import { expect } from "@playwright/test";

/** 已知允许的地图资源降级：本地字体缺失导致的 404。 */
const GLYPH_URL_MARKER = "/vendor/fonts/";

export function isAllowedGlyphDegradation(msg) {
  if (msg.kind !== "console.error") return false;
  return typeof msg.url === "string" && msg.url.includes(GLYPH_URL_MARKER);
}

/**
 * 挂载诊断采集器。返回的对象在测试结束时交给 assertNoDiagnostics。
 * 不做任何宽泛过滤——白名单匹配在断言处集中判断并计数。
 */
export function collectDiagnostics(page) {
  const entries = [];
  const warnings = [];
  page.on("pageerror", (e) => entries.push({ kind: "pageerror", text: String(e), url: "" }));
  page.on("console", (m) => {
    if (m.type() === "error") {
      entries.push({ kind: "console.error", text: m.text(), url: m.location()?.url ?? "" });
    } else if (m.type() === "warning") {
      warnings.push({ kind: "console.warn", text: m.text(), url: m.location()?.url ?? "" });
    }
  });
  // ws 计数器由测试自行经 addInitScript 注入；此处仅保持字段形状稳定
  return { entries, warnings, ws: { created: 0, closed: 0 } };
}

/** 断言除白名单外的诊断为空。返回被放行的降级条目数，便于报告。 */
export function assertCleanDiagnostics(diag, label) {
  const allowed = diag.entries.filter(isAllowedGlyphDegradation);
  const bad = diag.entries.filter((e) => !isAllowedGlyphDegradation(e));
  if (bad.length) {
    const dump = bad.map((e) => `  [${e.kind}] ${e.text}${e.url ? ` @ ${e.url}` : ""}`).join("\n");
    throw new Error(`${label}: 出现 ${bad.length} 条非白名单诊断\n${dump}`);
  }
  return allowed.length;
}

/** 打开正式页面并等待 MapLibre style 加载完成。 */
export async function gotoApp(page, origin) {
  await page.goto(`${origin}/`, { waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => map.loaded() === true, null, { timeout: 20_000 });
}

/** 等待 WS 连接建立（页面状态栏文案由 connect() 写入）。 */
export async function waitConnected(page) {
  await expect(page.locator("#conn-status")).toHaveClass(/on/, { timeout: 15_000 });
}

/** 通过真实 UI 设置目的地（填输入框 + 点按钮），返回 /api/route 的响应。 */
export async function setDestinationViaUi(page, { x, z }) {
  const resp = page.waitForResponse(
    (r) => r.url().includes("/api/route") && r.request().method() === "POST",
    { timeout: 20_000 },
  );
  await page.fill("#dest-x", String(x));
  await page.fill("#dest-z", String(z));
  await page.click("#btn-route");
  const r = await resp;
  expect(r.status()).toBe(200);
  return r;
}

/** 读取 MapLibre GeoJSON source 当前的坐标数组（经公开的 serialize()）。 */
export async function sourceCoordinates(page, sourceId) {
  return page.evaluate((id) => {
    const src = map.getSource(id);
    if (!src) return null;
    const data = src.serialize().data;
    if (!data) return null;
    return data.coordinates ?? null;
  }, sourceId);
}

/**
 * 把一条 §60 协议报文交给页面真实的 WS 消息处理入口。
 * 调用的是 app.js 顶层的 `dispatchMessage`（function 声明，即 window 属性），
 * 也就是 `ws.onmessage` 实际调用的同一个函数——因此 JSON 解析、类型分发、
 * DOM 渲染全部走生产代码，唯一未覆盖的是 socket 传输本身（由真实服务器的
 * 用例覆盖）。
 */
export async function deliverFrame(page, obj) {
  await page.evaluate((raw) => dispatchMessage(raw), JSON.stringify(obj));
}

/** 构造 §60 vehicle 帧（缺省字段可被 overrides 覆盖）。 */
export function vehicleFrame(overrides = {}) {
  return {
    type: "vehicle",
    state: "navigating",
    position: [-58400, 33000],
    speed_kmh: 50,
    map_limit_kmh: 50,
    remaining_m: 1234,
    remaining_s: 90,
    progress: 0.4,
    matched_edge: 1,
    next_maneuver: null,
    upcoming_signal: null,
    glosa: null,
    reminders: [],
    destination: "测试目标",
    diagnostics: "",
    ...overrides,
  };
}

/**
 * 断开页面的实时帧流，使后续的确定性注入不被 20 Hz 广播覆盖。
 *
 * 做法是把当前 socket 的处理器摘除后 close——`onclose` 被置空，因此不会触达
 * app.js 的重连排程（重连行为本身由 E2E-10 单独覆盖）。这只操作页面已有的公开
 * 全局，不改动生产代码语义。
 */
export async function detachStream(page) {
  await page.evaluate(() => {
    if (!ws) return;
    ws.onopen = ws.onmessage = ws.onerror = ws.onclose = null;
    try { ws.close(); } catch { /* 已关闭 */ }
  });
}

/**
 * 等待 #glosa 文本满足给定形态，**并返回被判定为匹配的那个值本身**。
 * mode ∈ range | single | empty | any。
 *
 * P4R Batch 5.5 §3：此前的实现是两次独立的浏览器往返——
 *   `waitForFunction(predicate)` 解出 → `locator.textContent()` 再读一次 DOM。
 * 二者之间存在 TOCTOU 窗口：谓词在帧 N 看到合法区间而解出，帧 N+1 把 #glosa
 * 清空，第二次读取拿到的就是 `""`。远端观测到的
 * 「waitGlosa 返回后 parseGlosa(shown) === null」正是这一形态。
 *
 * 现改为：谓词直接返回匹配到的字符串，`waitForFunction` 解出时携带的就是**那次
 * 判定的取值快照**（Playwright 对返回真值的求值结果取 JSHandle），此后不再读 DOM。
 * 判定与取值因此落在同一次 browser-context evaluation 内。
 *
 * 返回值用 `{ matched }` 包一层而不是直接返回字符串：`mode === "empty"` 的合法
 * 结果就是空串，而 `waitForFunction` 以真值作为「已满足」判据，直接返回空串会被
 * 当成未满足而永远等下去。
 */
export async function waitGlosa(page, mode, timeout = 30_000) {
  const handle = await page.waitForFunction((m) => {
    const t = document.getElementById("glosa").textContent;
    const ok = m === "range" ? /^建议 \d+–\d+ km\/h$/.test(t)
      : m === "single" ? /^建议 \d+ km\/h$/.test(t)
        : m === "empty" ? t === ""
          : true;
    return ok ? { matched: t } : false;
  }, mode, { timeout });
  const v = await handle.jsonValue();
  return String(v.matched).trim();
}

// ─── 帧证据采样（P4R Batch 5.5 §4/§5）────────────────────────────────────────

/**
 * `--fake-signal` 剧本的帧结构，必须与 `nav-router::session` 的常量一致。
 *
 * 之所以在测试侧复制这三个数：剧本的推进单位是**帧**而不是秒
 * （`fake_signal_phase(frame) = (frame / 40) % 6`），测试若用墙上时间描述等待
 * 窗口，就把「需要多少帧才能覆盖全部阶段」这一确定量换成了对 runner 速度的猜测。
 * 数值漂移会被 `fake_signal_script_matches_documented_glosa_values`（Rust 单测）
 * 先一步挡住，这里的副本只用于推导窗口大小。
 */
export const FAKE_SIGNAL_FRAMES_PER_PHASE = 40;
export const FAKE_SIGNAL_PHASE_COUNT = 6;
export const FAKE_SIGNAL_CYCLE_FRAMES = FAKE_SIGNAL_FRAMES_PER_PHASE * FAKE_SIGNAL_PHASE_COUNT;

/**
 * 覆盖全部信号状态所需的**帧**上界。
 *
 * 推导（不依赖测试何时进入剧本）：所需 DOM 状态对应阶段
 *   Red ∈ {0,1,5}、Yellow = {2}、None = {3}、Green = {4}
 * 从任意阶段出发，最坏情形是「刚错过 5（红）」。此时需依次经过
 *   2(Y) 3(N) 4(G) 5(R)  → 4 个阶段
 * 而若起点在阶段 5 之后（0），则需 0(R) 1 2(Y) 3(N) 4(G) → 5 个阶段。
 * 5 个阶段 × 40 帧 = 200 帧。取一个完整周期 240 帧作为上界，留 40 帧余量，
 * 同时使上界本身仍由剧本结构确定，而不是由观察到的耗时确定。
 */
export const FAKE_SIGNAL_COVER_FRAMES = FAKE_SIGNAL_CYCLE_FRAMES;

/**
 * 在页面脚本执行前登记采样器外壳（必须早于 `gotoApp`）。
 *
 * 采样点选在 app.js 顶层 `dispatchMessage` 上：`ws.onmessage` 的实现是
 * `dispatchMessage(ev.data)`（见 app.js `connect()`），即调用时才解析全局名，
 * 因此包住全局函数即可在**每一帧真实到达 WS 之后**、原函数返回的同一同步块内
 * 读取该帧产生的 DOM 契约。链路本身（fake-signal → session → snapshot_json →
 * WebSocket → dispatchMessage → DOM）完全不变，被覆盖的只有「读」这一步。
 */
export async function installFrameSampler(page) {
  await page.addInitScript(() => {
    window.__sigSamples = [];
    window.__sigInstall = () => {
      const orig = window.dispatchMessage;
      if (typeof orig !== "function") return "dispatchMessage 尚不存在";
      if (orig.__sampled) return true;
      const wrapped = function (raw) {
        const r = orig.call(this, raw);
        let d;
        try { d = JSON.parse(raw); } catch { return r; }
        if ((d.type || "vehicle") !== "vehicle") return r;
        const st = document.getElementById("signal-state");
        const card = document.getElementById("signal-card");
        window.__sigSamples.push({
          state: st.textContent.trim().toLowerCase(),
          hidden: card.classList.contains("hidden"),
          cls: st.className,
          remaining: document.getElementById("signal-remaining").textContent,
          glosa: document.getElementById("glosa").textContent,
          // 会话状态（#state-chip）。必须一起采集：§38 只在 `Navigating` 分支内
          // 计算，因此「红灯 ⇒ 有建议」这条蕴含只在会话确实在导航时成立；
          // 不带会话状态的样本无法区分「产品没算」与「会话本来就不在导航」。
          session: document.getElementById("state-chip").textContent.trim(),
        });
        return r;
      };
      wrapped.__sampled = true;
      window.dispatchMessage = wrapped;
      return true;
    };
  });
}

/**
 * 安装采样器并清空既有样本（在 `gotoApp` / `waitConnected` 之后调用）。
 * 返回安装结果字符串或 `true`；失败时调用方必须断言，不得静默继续。
 */
export async function startFrameSampling(page) {
  return page.evaluate(() => {
    window.__sigSamples = [];
    return window.__sigInstall();
  });
}

/** 读取当前累计样本（每个样本对应一个真实到达 WS 的 vehicle 帧）。 */
export async function readFrameSamples(page) {
  return page.evaluate(() => window.__sigSamples);
}

/**
 * 等到「所需状态全部出现」或「帧预算耗尽」为止。
 *
 * 完成条件以**收到的 vehicle 帧数**为准，因此与测试进入剧本的相位无关，也与
 * runner 的快慢无关：慢 runner 只会让同样的帧数花更长时间，不会让某个阶段被跳过
 * （服务端的帧间隔由 sim time 节流，只会变长不会变短）。
 * `timeout` 只是「流已死」的兜底，不承担相位对齐职责。
 */
export async function waitFrameEvidence(page, states, timeout) {
  const want = states.map((x) => x.toLowerCase());
  return page.waitForFunction(({ w, cap }) => {
    const s = window.__sigSamples;
    const seen = new Set(s.map((x) => x.state));
    if (w.every((x) => seen.has(x))) return "states-observed";
    if (s.length >= cap) return "frame-budget-exhausted";
    return false;
  }, { w: want, cap: FAKE_SIGNAL_COVER_FRAMES }, { timeout });
}

/**
 * **导航中**帧的证据摘要：帧数、各灯态出现次数、已出现的灯态集合。
 *
 * 用于 GLOSA 类用例。`--fake-signal` 的灯态由帧号决定，与产品是否在导航无关；
 * 而 §38 只在会话 `Navigating` 时计算，因此「红灯 ⇒ 有建议」的窗口必须以
 * **导航中的帧**计数，否则窗口会被 Idle 帧白吃掉，使判据退化为对会话状态的猜测。
 * 传输摘要而不是整个样本数组，避免每轮轮询都在 CDP 上传数百个对象。
 */
export async function navFrameEvidence(page) {
  return page.evaluate(() => {
    const nav = (window.__sigSamples || []).filter((x) => x.session === "NAVIGATING");
    const counts = {};
    for (const x of nav) counts[x.state] = (counts[x.state] || 0) + 1;
    return { nav: nav.length, counts, states: Object.keys(counts).sort() };
  });
}

/**
 * 失败诊断：一次性取回与信号/GLOSA 断言相关的全部可观测上下文。
 *
 * 目的是让失败信息本身能区分诊断方向相反的原因（元素文本没更新 / 卡片可见性
 * 错 / WS 已断开 / 服务端没在推进剧本 / 页面脚本抛错），而不是只给出一句
 * DOM 超时。**不包含任何令牌**：读取的都是页面可见文本、全局连接状态与计数器，
 * 令牌从不进入 DOM 文本。
 */
export async function dumpUiState(page) {
  try {
    return await page.evaluate(() => {
      const t = (id) => {
        const el = document.getElementById(id);
        return el ? el.textContent : `<无 #${id}>`;
      };
      const cls = (id) => {
        const el = document.getElementById(id);
        return el ? el.className : "<无>";
      };
      let wsState = "no-ws";
      try {
        const map = ["CONNECTING", "OPEN", "CLOSING", "CLOSED"];
        wsState = ws ? (map[ws.readyState] ?? String(ws.readyState)) : "null";
      } catch { wsState = "读取 ws 失败"; }
      const card = document.getElementById("signal-card");
      return {
        signalState: t("signal-state"),
        signalStateClass: cls("signal-state"),
        signalCardHidden: card ? card.classList.contains("hidden") : null,
        signalCardClass: card ? card.className : "<无>",
        signalRemaining: t("signal-remaining"),
        glosa: t("glosa"),
        stateChip: t("state-chip"),
        connStatus: t("conn-status"),
        wsReadyState: wsState,
        sampleFrames: Array.isArray(window.__sigSamples) ? window.__sigSamples.length : -1,
        lastSamples: Array.isArray(window.__sigSamples) ? window.__sigSamples.slice(-12) : [],
      };
    });
  } catch (e) {
    return { dumpError: String(e) };
  }
}

/** 解析 "建议 A–B km/h" / "建议 A km/h"；非法格式返回 null。 */
export function parseGlosa(text) {
  let m = /^建议 (\d+)–(\d+) km\/h$/.exec(text);
  if (m) return { min: Number(m[1]), max: Number(m[2]) };
  m = /^建议 (\d+) km\/h$/.exec(text);
  if (m) return { min: Number(m[1]), max: Number(m[1]) };
  return null;
}

/**
 * GLOSA 类用例的**导航目的地**（合成 trace 的终点）。
 * 与 `prepareTrace` 的 `-58456,32832:-52925,36510` 第二点一致。
 */
export const GLOSA_DEST = { x: -52925, z: 36510 };

/**
 * 采集「导航中」的帧证据，并在窗口内**维护前置条件**。
 *
 * 为什么必须维护：合成 trace 的回放循环每轮结束时 `session = NavigationSession::new(...)`
 * （见 server_cli.rs：A2c-M3，避免首轮 Arrived 后永久卡死），目的地随之丢失，会话回到
 * Idle；此外车辆驶到 trace 终点附近时会话会进入 Arrived。两种情况都使 §38 停止计算，
 * 与「测试开始时车辆恰好在 cycle 的哪一段」直接相关——这正是 P4R Batch 5.5 §5 要消除的
 * 隐含 wall-time coupling。此处不改动剧本、不加等待：会话一旦掉出 `NAVIGATING`，就经
 * **真实 UI 路径**（填坐标 + 点按钮）重新设目的地，使前置条件由测试自己维持。
 *
 * 窗口以**导航中的帧数**计量（`FAKE_SIGNAL_COVER_FRAMES`），因此与 runner 快慢和进入
 * 相位都无关。`deadlineMs` 是帧流已死时的兜底。
 *
 * `minCounts` 给出各灯态需要的最少帧数。它不是「多收一点更保险」，而是**非空断言
 * 的前提**：例如「绿灯期间 GLOSA 为空」只有在确实采到足够长的绿灯驻留期时才有
 * 区分力，否则采到 1 帧绿灯也会通过。剧本中绿灯固定占 40 帧，因此要求 20 帧必然
 * 在一个周期内可满足，不引入新的不确定性。
 */
export async function collectNavigatingEvidence(page, needStates, deadlineMs, minCounts = {}) {
  const want = needStates.map((x) => x.toLowerCase());
  const need = (counts) => want.every((x) => (counts[x] || 0) >= (minCounts[x] ?? 1));
  const end = Date.now() + deadlineMs;
  let reposts = 0;
  let last = { nav: 0, counts: {}, states: [] };
  for (;;) {
    last = await navFrameEvidence(page);
    if (need(last.counts)) return { reason: "states-observed", reposts, ...last };
    if (last.nav >= FAKE_SIGNAL_COVER_FRAMES) {
      return { reason: "frame-budget-exhausted", reposts, ...last };
    }
    if (Date.now() > end) return { reason: "deadline", reposts, ...last };
    const chip = ((await page.locator("#state-chip").textContent()) || "").trim();
    if (chip !== "NAVIGATING") {
      try {
        await setDestinationViaUi(page, GLOSA_DEST);
        reposts++;
      } catch { /* 页面尚未就绪：下一轮再试 */ }
    }
    await page.waitForTimeout(150);
  }
}
