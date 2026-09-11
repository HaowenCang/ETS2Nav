// Playwright global setup（P4R Batch 2 §10）：建立隔离测试环境一次，供全部 spec 复用。
//
// 关键点：测试用的 PMTiles 由正式 PmtilesWriter 现场生成，服务器端口动态分配，
// web root 位于临时目录；开发者源码目录中的 tools/ets2nav-web/map.pmtiles 不参与。

import { cleanupSession, prepareTrace, prepareWebRoot, startServer, writeSessionInfo } from "./harness.mjs";

export default async function globalSetup() {
  await cleanupSession();

  const { webRoot, fixtureBytes } = await prepareWebRoot();
  const trace = await prepareTrace();

  // 两个服务器实例：
  //   data   —— 不注入合成信号，用于地图/车辆/路线/分发等真实数据断言；
  //   signal —— 注入确定性灯态剧本，用于信号与 GLOSA 卡片断言
  //             （离线回放没有实机灯态，§36/§37/§38 否则不可达）。
  const data = await startServer({ webRoot, trace });
  const signal = await startServer({ webRoot, trace, fakeSignal: true });

  await writeSessionInfo({
    webRoot,
    fixtureBytes,
    trace,
    dataOrigin: data.origin,
    dataPort: data.port,
    signalOrigin: signal.origin,
    signalPort: signal.port,
  });

  // 进程随 Playwright 退出；全局 teardown 负责正常路径的清理。
  globalThis.__ets2navServers = [data, signal];
}
