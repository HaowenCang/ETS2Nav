// Playwright 配置（P4R Batch 2 §9）。
//
// 环境约束：只使用 Chromium；不使用开发机全局 Playwright（版本由 package.json /
// package-lock.json 锁定，@playwright/test 1.63.0）。E2E 必须启动真实 Chromium 并
// 真实加载 index.html + app.js + MapLibre + pmtiles protocol + QRCode + HTTP + WS，
// 不是 jsdom。
//
// 本机缓存中曾存在更旧的 chromium-1223/1228（其他项目下载），故显式关闭「自动使用
// 已存在的浏览器」，确保运行的是本仓库锁定版本对应的浏览器构建。

import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  globalSetup: "./tests/global-setup.mjs",
  globalTeardown: "./tests/global-teardown.mjs",
  // 测试之间共享服务器进程，但每个 test 使用独立 browser context（默认隔离）。
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: [["list"]],
  timeout: 60_000,
  expect: { timeout: 10_000 },
  use: {
    ...devices["Desktop Chrome"],
    channel: undefined,
    // 关闭「复用已安装浏览器」：只用本仓库锁定版本下载的构建
    //
    // `--no-proxy-server`：LAN 用例从 http://<RFC1918>:port/ 打开页面，以此让服务端
    // 看到**私网**对端（本机连自己的私网地址时源地址即该私网地址）。若浏览器把请求
    // 交给系统代理，服务端看到的对端会变成回环并从豁免路径通过，用例会悄然测错对象。
    // 本机系统代理的 bypass 列表恰好覆盖 10.*/172.16-31.*/192.168.*，但那是巧合而非
    // 保证，故在此显式禁用代理，使结果不依赖开发机的代理配置。
    launchOptions: {
      args: ["--no-sandbox", "--disable-dev-shm-usage", "--no-proxy-server"],
    },
    actionTimeout: 10_000,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
});
