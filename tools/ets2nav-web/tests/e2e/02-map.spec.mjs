// E2E-02 地图初始化（P4R Batch 2 §11）。
//
// 必须验证：MapLibre load 完成、map.loaded() 为 true、tiles source 为 vector 且
// 指向 pmtiles://、vehicle/route-line source 存在、road 图层存在，并且**至少一个
// 矢量瓦片被真正读取**——不能只检查 source/layer 对象被创建。
//
// 「真正读取」的判据有两条独立证据：
//   1. HTTP 层收到 /map.pmtiles 的 206 分片响应（PMTiles 协议即按 Range 取瓦片）；
//   2. MapLibre 的 querySourceFeatures 在 tiles source 上返回非空要素集——
//      这要求瓦片已被下载、解压、解析并进入瓦片索引。

import { expect, test } from "@playwright/test";
import { gotoApp } from "./helpers.mjs";
import { session } from "../harness.mjs";

test("E2E-02 地图加载并使用 PMTiles 矢量源", async ({ page }) => {
  const s = await session();
  const tileResponses = [];
  page.on("response", (r) => {
    if (r.url().includes("/map.pmtiles")) {
      tileResponses.push({ status: r.status(), range: r.request().headers()["range"] ?? null });
    }
  });

  await gotoApp(page, s.dataOrigin);

  // tiles source：vector + pmtiles:// URL（这是 P4R Batch 1 修复后的正确接入形态）
  const tiles = await page.evaluate(() => map.getSource("tiles")?.serialize() ?? null);
  expect(tiles, "tiles source 必须存在").not.toBeNull();
  expect(tiles.type).toBe("vector");
  expect(tiles.url).toBe("pmtiles://map.pmtiles");

  // vehicle / route-line source 与 road 图层
  const state = await page.evaluate(() => ({
    sources: Object.keys(map.getStyle().sources),
    layers: map.getStyle().layers.map((l) => l.id),
    roadLayer: map.getLayer("tile-road") ? map.getLayer("tile-road").type : null,
  }));
  expect(state.sources).toContain("vehicle");
  expect(state.sources).toContain("route-line");
  expect(state.layers).toContain("tile-road");
  expect(state.roadLayer).toBe("line");

  // 证据 1：瓦片经 HTTP Range 分片取回（206）
  await expect
    .poll(() => tileResponses.filter((r) => r.status === 206).length, { timeout: 20_000 })
    .toBeGreaterThan(0);
  expect(tileResponses.some((r) => r.range !== null), "取瓦片必须带 Range 头").toBe(true);

  // 证据 2：瓦片已被解析进瓦片索引。
  // 注意 querySourceFeatures 必须带 sourceLayer——不带时 MapLibre 4.7.1 返回 0
  // （实测：不带 0 条，带 "road" 9 条），会造成「瓦片没加载」的假阴性。
  await expect
    .poll(async () => page.evaluate(() => map.querySourceFeatures("tiles", { sourceLayer: "road" }).length), {
      timeout: 20_000,
    })
    .toBeGreaterThan(0);

  // 证据 3：road 图层真的被绘制（结构化要素查询，非像素统计）
  await expect
    .poll(async () => page.evaluate(() => map.queryRenderedFeatures({ layers: ["tile-road"] }).length), {
      timeout: 20_000,
    })
    .toBeGreaterThan(0);
});

test("E2E-02b 缺少字体时只跳过 city 文字层（唯一允许的地图资源降级）", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  const layers = await page.evaluate(() => map.getStyle().layers.map((l) => l.id));
  // 几何图层必须全部就位
  expect(layers).toContain("tile-road");
  expect(layers).toContain("tile-junction");
  expect(layers).toContain("tile-poi");
  // city 文字层依赖本地 glyphs；缺失时按既定降级跳过（不得因缺字体而丢失几何图层）
  const hasFonts = await page.evaluate(async () =>
    (await fetch("vendor/fonts/Open%20Sans%20Regular/0-255.pbf", { method: "HEAD" })).ok);
  if (hasFonts) {
    expect(layers).toContain("tile-city");
  } else {
    expect(layers).not.toContain("tile-city");
  }
});
