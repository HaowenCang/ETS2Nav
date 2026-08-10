# P2-17 Navigation Session 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §118-121（状态机/协调/快照/headless）。
**产物**：nav-router session 模块（NavigationSession + NavigationSnapshot）+ CLI `session` 命令。

## 一、实现

### 状态机（§118）
```
Idle → DestinationSet → Planning → Navigating
Navigating ⇄ SuspectedOffRoute（detector 证据）→ Rerouting → Navigating（重规划）
Navigating → Arrived（剩余 < 30m）
Navigating → Paused（低速 > 阈值帧）｜ LostPosition（matcher 丢失，P2-06 语义）
任何失败 → Error
```

### 协调（§119）
每帧管线：TelemetrySnapshot → MapMatcher → RouteTracker（窗口+反向对边识别）→ RerouteDetector →（确认偏航时 plan）→ ManeuverGenerator（next_maneuver）→ SignalLinker（next_signal）→ NavigationSnapshot

### NavigationSnapshot（§120）
state/position/matched_edge/confidence/route 距离/remaining/progress/next_maneuver/upcoming_signal/destination/diagnostics——未来 UI（WebSocket/Mobile）统一源；**headless 无 UI**（§121）

## 二、关键修复

- **双向路对向边抖动**：matcher 可能选中 route 边的对向边（同几何）——tracker 窗口不匹配 → 误判偏航。
  修复：窗口内**反向对边识别**（geom_start/len 相同 + kind 相同）→ matched 但**不推进进度**（§87 语义）
- **纯虚拟段路线**（起点/终点都在边中部——无中间图边）：tracker 空 edges 崩溃 → 剩余=全程距离、匹配终点边归零
- 到达判定统一用 update 返回值（含虚拟段语义）

## 三、验证

### 单元测试（2 个，总计 32 全绿）
- 沿路线行驶 → Idle → Navigating → **Arrived**（remaining < 30m）
- 低速持续 → **Paused**（阈值可配置）

### Europe v4 真实会话（Berlin 2.26km 路线，652 帧插值驱动）
```
帧 0: → Navigating（matched=High 剩余=2263m progress=0.00）
帧 215: → Navigating（matched=High 剩余=1718m progress=0.24）
帧 306: → SuspectedOffRoute → 313 恢复（matched=High progress=0.40）
帧 615: → Arrived（matched=High 剩余=30m progress=0.99）
到达（帧 615）——状态机闭环验证 PASS
```
- 全程 High 置信度、progress 单调 0→0.99、偶发 Suspected 均恢复（hysteresis + 反向对边识别生效）

## 四、门

- fmt / clippy 0 / 32 测试全绿；P1 Regression 不受影响

## 五、下一步

- P2-18 Europe regression：全图正确性回归（随机 OD 搜索 + 全链路基准）
- P2-19 Performance/closeout：性能基准（§139-140）+ 4 子代理审查 + 关门报告 + tag v0.3.0-p2
