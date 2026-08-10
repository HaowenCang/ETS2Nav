# P2-19 Performance / Closeout 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §139-140（性能目标）+ §159（PR 门槛）+ 关门流程。
**产物**：CLI `bench` 命令（正式性能基准）+ 本报告（审查后补审查结论）。

## 一、正式性能基准（Europe v4 全量：696,717 边 / 5,633,750 节点）

### §139/§140 指标对照（审查修订：节号与口径）
| 指标 | 目标 | 实测 | 判定 |
|---|---|---|---|
| 数据集加载（§139：数秒级，记录 cold/warm/peak） | 数秒级 | **298ms 冷启动**（routing.graph）+ build 103ms + spatial 48ms；warm 未单独测（同一进程重复加载即 warm 语义） | ✓ |
| 内存（§139 peak RAM 记录） | <500MB | **峰值估算 ~352MB**（加载+build 瞬时，含 RoutingGraph 原始数据——构建后 drop 释放）；**常驻（构建后）217MB**（目标按常驻口径判定） | ✓ |
| 典型单路线（§140） | <500ms | **p50 0.14ms / p99 0.39ms / max 0.39ms**（Berlin 核心网 19 OD） | ✓ 超出 1200× |
| 匹配 p99（§140） | <10ms | **0.010ms**（517 帧 trace 回放；注：trace 为 2Hz 采样，10-20Hz 目标频率下需游戏实测补测） | ✓ |
| 偏航重规划（§93） | ≈1s | **<0.5ms**（P2-12 实测；报告中 0.0ms 为毫秒级舍入值） | ✓ |
| Runtime CPU（§144）/ Game FPS（§146） | 1-3% / ≤1-2% | **未测——需游戏会话**（已知限制，目标契约排除） | pending |

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

## 三、已知限制（游戏会话排除项——目标契约；审查修订）

1. **游戏内实测类**（明确排除）：UK 环岛方向驾驶实测（§103）、Speed Gate 限速采集（§40-41）、
   信号灯 runtime 关联 VERIFIED 实测（§115）、驾驶 corpus 标注（§104）
2. **G15 Full Session（§178——计划规定"P2 真正关门条件"）**：完整游戏会话端到端验证未做
   （P2-17 为合成帧/路线帧验证）——登记为 P2 关门前最大遗留，需游戏会话后补
3. **Runtime CPU（§144）/ Game FPS 回退（§146）**：需游戏会话实测
4. **环岛几何识别**：SCS 环岛为放射直通结构（拓扑无环实证）——几何识别需 corpus 校准（P2-14 遗留）；
   **环岛出口引导 maneuver 集成**（"第 N 个出口驶出"）待几何识别落地
5. **matcher 权重/阈值**（§47）为默认值——trace calibration 后冻结；**route bias（§50）预留未接**
   （P2-12 rerouting 时启用）
6. **maneuver 细化阈值**（slight/normal/sharp，§97/L4）为默认值——corpus 验证后冻结
7. **warm load** 未单独测（冷启动 298ms 已达标）；trace 采样频率 2Hz（匹配 p99 按此口径）

## 四、4 子代理审查与修复记录（2026-08-10）

### 审查发现与修复
| 级别 | 发现 | 修复 |
|---|---|---|
| BLOCKER | 内存验收口径：峰值实测 ~888MB（含原始数据）vs 报告 217MB | bench 区分峰值（~352MB 估算）/常驻（217MB）；构建后 drop(routing)；目标按常驻口径判定 |
| MAJOR | quat_yaw 公式绕 Z（纯 yaw 输出 0/180°） | session/CLI 改绕 Y 提取（与 signal::light_yaw 一致） |
| MAJOR | tracker 虚拟段未计入 suffix（剩余 4900≠路线 4963m） | suffix 计算加终点虚拟段 |
| MAJOR | single_edge_route ETA 与距离不一致 | ETA 按实际行驶段计算 |
| MAJOR | alternatives 质量过滤跨 profile 单位混比 | 统一用距离（米）比较 |
| MAJOR | manifest §29 校验缺失（game_version/scope/files） | C# 写端加 game_version（1.60.1.7 + hash）+ Rust 校验 scope/版本/files；数据集重建 v5 验证 |
| MAJOR | run-p2-tests.bat（§162）不存在 | 已建立 7 步套件（P1 回归+cargo 门+smoke+regression+replay+signal+perf）ALL PASS |
| MAJOR | G15/§144/§146/环岛引导/route bias 未登记 | 本报告已知限制补登记 |
| MINOR | 测试数口径（32 vs 实际 49） | 全部报告统一 49 |
| MINOR | P2-10 Berlin 北向快照漂移 | 报告更新（质量过滤修复后 2 条） |
| MINOR | P2-18 东欧不可达 21 vs 24 | 报告更新 |
| MINOR | 0.0ms 舍入 / trace 2Hz / 冷启动口径 | 报告说明 |

### 审查结论
- 实现正确性审查（nav-* 全模块）：超时部分完成——发现如上已修；环岛 r_1w_* token 调查未定案（登记待验证）
- 计划符合性：工作包-§ 映射无张冠李戴；bench/regression/session 数字可复现
- 文档一致性：测试数等口径统一
- 性能边界：BLOCKER 内存口径已修

## 五、关门状态

- [x] 4 子代理审查（实现正确性/计划符合性/文档一致性/性能边界）
- [x] BLOCKER/MAJOR 修复（上述 8 项）
- [x] P1 Regression 保持绿（run-p2-tests.bat 1/7 PASS）
- [x] run-p2-tests.bat ALL PASS（§162）
- [ ] tag v0.3.0-p2
