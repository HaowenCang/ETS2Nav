// Playwright global setup（P4R Batch 2 §10）：建立隔离测试环境一次，供全部 spec 复用。
//
// 关键点：测试用的 PMTiles 由正式 PmtilesWriter 现场生成，服务器端口动态分配，
// web root 位于临时目录；开发者源码目录中的 tools/ets2nav-web/map.pmtiles 不参与。

import {
  cleanupSession, lanCandidates, prepareTrace, prepareWebRoot, startServer, writeSessionInfo,
} from "./harness.mjs";

export default async function globalSetup() {
  await cleanupSession();

  const { webRoot, fixtureBytes } = await prepareWebRoot();
  const trace = await prepareTrace();

  // 三个服务器实例：
  //   data   —— 不注入合成信号，用于地图/车辆/路线/分发等真实数据断言；
  //   signal —— 注入确定性灯态剧本，用于信号与 GLOSA 卡片断言
  //             （离线回放没有实机灯态，§36/§37/§38 否则不可达）；
  //   lan    —— 额外加 `--lan`，用于令牌/二维码/同源 WS 的用例。
  //
  // data / signal 也以 `--lan` 启动：回环对端在 LAN 模式下**豁免令牌**，因此既有
  // 26 项用例的访问方式与断言完全不变，同时二维码拿到真实局域网地址（不再编码
  // 无效的 127.0.0.1），使 E2E-11b 的二维码布局断言保持有意义。
  const data = await startServer({ webRoot, trace, lan: true });
  const signal = await startServer({ webRoot, trace, fakeSignal: true, lan: true });
  const lan = await startServer({ webRoot, trace, fakeSignal: true, lan: true });

  // 候选地址在 setup 阶段取一次：端口已知，且测试机网卡不应在会话中途变化。
  const candidates = await lanCandidates("127.0.0.1", lan.port);

  await writeSessionInfo({
    webRoot,
    fixtureBytes,
    trace,
    dataOrigin: data.origin,
    dataPort: data.port,
    signalOrigin: signal.origin,
    signalPort: signal.port,
    lanOrigin: lan.origin,
    lanPort: lan.port,
    lanCandidates: candidates,
  });

  // 进程随 Playwright 退出；全局 teardown 负责正常路径的清理。
  globalThis.__ets2navServers = [data, signal, lan];
}
