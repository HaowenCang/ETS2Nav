# ETS2Nav — Euro Truck Simulator 2 外部智能导航系统

独立于游戏原生 Route Advisor 的外部智能导航系统：自行解析 ETS2 地图资源建立导航数据库，提供独立路径规划、实时地图匹配、转向导航、红绿灯倒计时（±1 s 目标）、限速与测速提示、POI 搜索、中文语音导航，PC 核心 + PC/移动端双前端。

- **需求与技术基线**：[Euro Truck Simulator 2 外部智能导航系统-v0.2.md](./Euro%20Truck%20Simulator%202%20外部智能导航系统-v0.2.md)
- **可行性评估**：[ETS2 外部智能导航系统可行性评估.md](./ETS2%20外部智能导航系统可行性评估.md)
- **执行计划与进度**：[PLAN.md](./PLAN.md)
- **当前阶段**：**P0~P6 A 侧全部关门（2026-08-12，tag `v0.6.0-p4p5p6` @ `7900bbc`）**——P3 关门 tag v0.4.0-p3；A2~A5（P4 UI / P5 OD corpus / P6 性能评估 / B7 工具）tag v0.5.0；此后完成 **P5 图缺陷根因排查与修复**（UK 孤立与 ferry 悬空为真实编译器缺陷——主分量 75.78%→79.35%、transit 端点孤立 258→0、od-check uturns 1→0），run-p1/p2/p3/p5 四套件 ALL PASS。规范数据集 `data/europe-v5`；**唯一剩余工作为 B 侧实机测试**（各阶段实机项登记为已知限制）。
- **关门报告**：P4 / P5 / P6 见 [docs/validation/](./docs/validation/)（`p4-closeout` / `p5-closeout` / `p6-closeout-2026-08.md`）；P5 修复根因与验证见 `p5-graph-defects-2026-08.md` / `p5-graph-defects-verification-2026-08.md`

## 架构概览（v0.2 §3）

```
ETS2 ──SCS Telemetry SDK──▶ Telemetry Bridge (C++ DLL) ──Shared Memory──▶ Navigation Core (Rust)
                                                                              │ HTTP/WebSocket
                                                                              ▼
                                                                  Windows UI / Mobile / Browser

ETS2 Files ──▶ Map Compiler (C#/TS，离线低频) ──▶ map.db / routing.graph / junction.graph / search.db / map.pmtiles
```

## 开发状态

见 [PLAN.md §3 任务状态表](./PLAN.md)。

## 目录结构（P0 精简版，完整版见 v0.2 §72）

```
ets2-nav/
├─ telemetry-plugin/   # C++ DLL：SDK 1.14 + 共享内存
├─ map-compiler/       # 地图解析与编译（独立实现，不复制 GPL 代码）
├─ tools/              # P0/P1 工具：map-inspector / graph-debugger / telemetry-dump / signal-lab / perf-bench
├─ map-compiler/tests/  # 自动化测试（8 测试项目，65 测试）
├─ docs/               # 格式笔记、决策记录
└─ PLAN.md             # 执行计划与进度
```

## 许可证

**GPL-3.0**（2026-08-09 决定：项目改为 GPL 开源，以允许复用 GPL 生态实现——TruckLib/ETS2LA/TruckSim Maps）。

历史：v0.1 阶段曾定为 MIT；2026-08-09 修订为 GPL-3.0。
