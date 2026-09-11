// Playwright global teardown：停止测试服务器并删除临时目录。
// 只删除本会话在系统临时目录下创建的目录（harness.SESSION_DIR），不触碰仓库内
// 任何文件，也不触碰用户已有文件。

import { cleanupSession } from "./harness.mjs";

export default async function globalTeardown() {
  for (const s of globalThis.__ets2navServers ?? []) {
    try { await s.stop(); } catch { /* 进程可能已退出 */ }
  }
  await cleanupSession();
}
