// E2E-01 页面启动（P4R Batch 2 §11）。
//
// 断言：无 pageerror、无未捕获异常、无非白名单 console.error。
// 本地字体 404 是 P4R Batch 1 确立的已知降级，允许出现，但必须精确匹配
// （/vendor/fonts/ 路径），不得用宽泛 error filter 掩盖其他问题。

import { expect, test } from "@playwright/test";
import { assertCleanDiagnostics, collectDiagnostics, gotoApp, waitConnected } from "./helpers.mjs";
import { session } from "../harness.mjs";

test("E2E-01 页面启动无未捕获异常与非法 console.error", async ({ page }) => {
  const s = await session();
  const diag = collectDiagnostics(page);
  await gotoApp(page, s.dataOrigin);
  await waitConnected(page);

  // 页面骨架与三个脚本（MapLibre / pmtiles / QRCode）都已就绪
  await expect(page.locator("#map")).toBeVisible();
  const libs = await page.evaluate(() => ({
    maplibregl: typeof maplibregl,
    pmtiles: typeof pmtiles,
    qrcode: typeof QRCode,
  }));
  expect(libs.maplibregl).toBe("object");
  expect(libs.pmtiles).toBe("object");
  expect(libs.qrcode).toBe("object");

  // 应用自身的错误上报通道未被触发（logError 一律 console.error 前缀 [ets2nav]）
  const appErrors = diag.entries.filter((e) => e.text.includes("[ets2nav]"));
  expect(appErrors, `应用自身报错：${JSON.stringify(appErrors)}`).toHaveLength(0);

  const allowed = assertCleanDiagnostics(diag, "E2E-01");
  // 只允许 fonts 探测这一项降级；出现其他任何诊断即为失败
  expect(allowed).toBeLessThanOrEqual(1);
});

test("E2E-01b 页面暴露真实协议处理入口（防止白名单掩盖脚本未加载）", async ({ page }) => {
  const s = await session();
  await gotoApp(page, s.dataOrigin);
  const kinds = await page.evaluate(() => ({
    dispatchMessage: typeof dispatchMessage,
    onSnapshot: typeof onSnapshot,
    onMapState: typeof onMapState,
    formatGlosa: typeof formatGlosa,
    mapIsMap: typeof map.getStyle === "function",
  }));
  expect(kinds.dispatchMessage).toBe("function");
  expect(kinds.onSnapshot).toBe("function");
  expect(kinds.onMapState).toBe("function");
  expect(kinds.formatGlosa).toBe("function");
  expect(kinds.mapIsMap).toBe(true);
});
