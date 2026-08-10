# P2-12 Rerouting 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §88-93（hysteresis/证据/四态/重规划/worker/1s gate）。
**产物**：nav-router reroute 模块 + CLI 偏航-重规划模拟。

## 一、实现

### Off-route Detection（§88-89）
- **hysteresis**：不因 `matched_edge != route_edge` 立即 reroute——多帧证据
- 证据组合：连续窗口外匹配帧数 + 沿非 route 边前进距离（§89：`min_speed_dist 0.5m` 以下不计入——静止不误报）+ 恢复可能（matched 即重置）

### 状态机（§90）
```
ON_ROUTE →(5 帧 unmatched)→ SUSPECTED →(15 帧 或 80m 距离)→ OFF_ROUTE
任何状态 matched → ON_ROUTE（恢复）
OFF_ROUTE → begin_rerouting → REROUTING → reset → ON_ROUTE
```

### Rerouting（§91/93）
- `reroute(graph, router, current_snap, destination, profile)`：当前 SnapPoint + 同目的地 + 同 profile → 新路线
- §92 worker thread 语义：Router 无状态冲突（generation counter），P2-17 session 接入线程

## 二、验证

### 单元测试（3 个，总计 25 全绿）
- 状态转换：4 帧 OnRoute → 第 5 帧 Suspected → 恢复清零 → 帧数确认 OffRoute
- 距离证据：30m/帧 × 3 帧 = 90m > 80m → OffRoute（无需 15 帧）
- 慢速（0.4m < min_speed）：距离不累计、仅 Suspected（防静止误报）

### Europe v4 真实模拟（Berlin 路线）
```
状态序列：40 帧 ON_ROUTE → 偏航后第 4 帧（4×20m=80m 距离证据）→ OFF_ROUTE
重规划成功：3613m 240s 56 边 0.0ms（§93 目标 ≈1s——远超）
检测器已重置: ON_ROUTE
```

## 三、门

- fmt / clippy 0 / 25 测试全绿
- 距离证据优先于帧数证据的设计确认（快速偏航 4 帧即确认）

## 四、下一步

- P2-13 Maneuver generator：Route Edge Sequence → Maneuver[]（TurnType 优先 + 几何细化 slight/normal/sharp + 抑制规则 + Keep Left/Right，§94-99）
- P2-14 Roundabout/Transit：环岛 exit 计数引导 + Ferry/Train leg（§100-106）
