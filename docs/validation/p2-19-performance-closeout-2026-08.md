# P2-19 Performance / Closeout 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §139-140（性能目标）+ §159（PR 门槛）+ 关门流程。
**产物**：CLI `bench` 命令（正式性能基准）+ 本报告（审查后补审查结论）。

## 一、正式性能基准（Europe v4 全量：696,717 边 / 5,633,750 节点）

### §139-140 指标对照
| 指标 | 目标 | 实测 | 判定 |
|---|---|---|---|
| 数据集加载 | — | **280ms**（routing.graph）+ build 108ms + spatial 48ms = 436ms 冷启动 | ✓ |
| 内存（粗略） | <500MB | **217MB**（节点/边/几何/CSR 邻接） | ✓ |
| 典型单路线 | <500ms | **p50 0.14ms / p99 0.38ms / max 0.38ms**（Berlin 核心网 19 OD） | ✓ 超出 1300× |
| 匹配 p99 | <10ms | **0.010ms**（517 帧 trace 回放） | ✓ 超出 1000× |
| 偏航重规划 | ≈1s | **0.0ms**（P2-12 实测） | ✓ |

### 命令
```
nav-core-cli bench <dataset-dir>          # 本报告全部数字
nav-core-cli regression <dataset-dir>     # 区域化全图回归（P2-18）
nav-core-cli route <x1,z1:x2,z2> <dir>    # 三 profile + 备选 + maneuver
nav-core-cli session <trace> <dir>        # 状态机闭环
```

## 二、P2 交付物总览（18 工作包）

| 包 | 核心产物 | 关键验证 |
|---|---|---|
| P2-00~02 | Dataset v2 / Rust runtime | Europe v4 全量加载 436ms / CSR 校验 PASS |
| P2-03 | Telemetry/trace | P0 真实 trace 517 帧 0 误报 |
| P2-04 | 前置验证 | sec- sector 修复（UK 422 sector）/ LHT 镜像 / UK OD 93.7% |
| P2-05/06 | Spatial / Matcher | 4µs 查询 / Berlin trace 86% HIGH+MED |
| P2-07~10 | Snap/Cost/A*/Alternatives | A*==Dijkstra 0 不一致 / London 2 备选 |
| P2-11/12 | Tracker/Rerouting | progress 单调 1.000 / 偏航 4 帧确认重规划 0.0ms |
| P2-13/14 | Maneuver/环岛 | 39 条/80 边 / 拓扑检测+出口编号（SCS 环岛放射结构实证） |
| P2-15~17 | Dest/Signal/Session | POI access_node 解析 / 信号静态绑定 / 状态机闭环 Arrived |
| P2-18/19 | Regression/基准 | 30/54 一致 0 不一致 / 全部指标达标 |

## 三、已知限制（游戏会话排除项——目标契约）

1. **游戏内实测类**（明确排除）：UK 环岛方向驾驶实测（§103）、Speed Gate 限速采集（§40-41）、
   信号灯 runtime 关联 VERIFIED 实测（§115）、驾驶 corpus 标注（§104）
2. **环岛几何识别**：SCS 环岛为放射直通结构（拓扑无环实证）——几何识别需 corpus 校准（P2-14 遗留）
3. **CPU/游戏 FPS 回退**（§140）需运行时实测
4. **matcher 权重/阈值**（§47）为默认值——trace calibration 后冻结（游戏会话）

## 四、关门流程

- [ ] 4 子代理审查（实现正确性/计划符合性/文档一致性/性能边界）
- [ ] BLOCKER/MAJOR 修复
- [ ] P1 Regression 保持绿
- [ ] tag v0.3.0-p2
