// E2E-04 路线（P4R Batch 2 §11）。
//
// 通过真实 UI 操作设置目的地（填输入框 + 点「设目的地」按钮，而不是直接调用内部
// 函数）。
//
// 路线几何有两条独立的到达路径，必须分开验证，否则一条路径失效会被另一条掩盖：
//   a) HTTP 响应路径——setRoute 消费 POST /api/route 的 polyline；
//   b) WS map_state 路径——服务端主动广播的路线几何。
// 测试 a 时先摘除实时帧流，使观测到的折线只能来自 HTTP 响应。

import { expect, test } from "@playwright/test";
import { detachStream, gotoApp, setDestinationViaUi, sourceCoordinates, waitConnected } from "./helpers.mjs";
import { session } from "../harness.mjs";

test("E2E-04a HTTP 响应路径渲染路线几何", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  await detachStream(page);

  const resp = await setDestinationViaUi(page, { x: -52925, z: 36510 });
  const body = await resp.json();
  expect(Array.isArray(body.polyline), "响应必须含 polyline").toBe(true);
  expect(body.polyline.length).toBeGreaterThan(100);

  await expect
    .poll(async () => (await sourceCoordinates(page, "route-line"))?.length ?? 0, { timeout: 20_000 })
    .toBeGreaterThan(1);

  // 折线即响应内容（渲染层坐标 = ETS2 米 / 111320）
  const coords = await sourceCoordinates(page, "route-line");
  expect(coords.length).toBe(body.polyline.length);
  const K = 111320;
  expect(coords[0][0]).toBeCloseTo(body.polyline[0][0] / K, 9);
  expect(coords[0][1]).toBeCloseTo(body.polyline[0][1] / K, 9);
  const last = coords.length - 1;
  expect(coords[last][0]).toBeCloseTo(body.polyline[last][0] / K, 9);
  expect(coords[last][1]).toBeCloseTo(body.polyline[last][1] / K, 9);
});

test("E2E-04b WS map_state 路径渲染路线几何", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);

  // 判据设计（本用例初版曾对 mutation B 失效，见 Batch 2 报告）：
  //   「先清空再断言非空」只有在清空之后**页面自身的 setRoute 路径不再被触发**时才
  //   有区分力。因此这里：
  //     1. 用真实 UI 的「清除」按钮把折线置空（btn-reset 直接 setData([])，是独立
  //        于 onMapState 的代码路径）；
  //     2. 用 fetch 直接 POST /api/route —— 页面看不到该响应，其 setRoute 不会被调用；
  //     3. 此时折线若重新变为非空，唯一可能的来源就是服务端广播的 map_state
  //        → onMapState。
  //   onMapState 被破坏（mutation B）时折线恒为空，poll 必然超时。
  await page.click("#btn-reset");
  await expect.poll(async () => (await sourceCoordinates(page, "route-line"))?.length ?? -1).toBe(0);

  // 经页面自身的 apiFetch 发出（Batch 3.5：动态 API 一律要求会话令牌，页面已由
  // 回环 bootstrap 取得；测试不注入旁路凭据，走的正是真实客户端路径）。
  const posted = await page.evaluate(async () => {
    const r = await apiFetch("/api/route", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ from: [-58456, 32832], to: [-52925, 36510] }),
    });
    return r.status;
  });
  expect(posted).toBe(200);

  await expect
    .poll(async () => (await sourceCoordinates(page, "route-line"))?.length ?? 0, { timeout: 25_000 })
    .toBeGreaterThan(1);

  // 折线内容必须是 ETS2 世界坐标换算后的经纬度（渲染层约定 lng = x / 111320），
  // 且各点互不相同——排除「写入了退化折线」这种假通过。
  const coords = await sourceCoordinates(page, "route-line");
  const uniq = new Set(coords.map((c) => `${c[0]},${c[1]}`));
  expect(uniq.size).toBeGreaterThan(1);
  for (const c of coords.slice(0, 5)) {
    expect(Math.abs(c[0])).toBeLessThan(1);
    expect(Math.abs(c[1])).toBeLessThan(1);
  }
});

test("E2E-04c 起点不可路由时返回 4xx 且不渲染折线", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);
  await detachStream(page);

  // 远离任何道路的坐标 → snap_nearest 失败 → 404
  const resp = await page.evaluate(async () => {
    const r = await apiFetch("/api/route", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ from: [9999999, 9999999], to: [9999999, 9999999] }),
    });
    return { status: r.status };
  });
  expect(resp.status).toBeGreaterThanOrEqual(400);
  expect((await sourceCoordinates(page, "route-line"))?.length ?? 0).toBeLessThanOrEqual(1);

  // 声明为 JSON 但内容非法 → 400
  const bad = await page.evaluate(async () => {
    const r = await apiFetch("/api/route", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    });
    return r.status;
  });
  expect(bad).toBe(400);

  // Batch 3.5：未声明 application/json 的写入请求在鉴权之后、解析之前被 415 拒绝
  // （fetch 对字符串 body 默认填 text/plain;charset=UTF-8，正是 CSRF 可用的形态）
  const wrongType = await page.evaluate(async () => {
    const r = await apiFetch("/api/route", { method: "POST", body: "{}" });
    return { status: r.status, sent: r.headers.get("content-type") };
  });
  expect(
    wrongType.status,
    "text/plain body 不得被当作 JSON 执行",
  ).toBe(415);
});
