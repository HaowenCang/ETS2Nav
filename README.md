# ETS2Nav — Euro Truck Simulator 2 外部智能导航系统

独立于游戏原生 Route Advisor 的外部智能导航系统：自行解析 ETS2 地图资源建立导航数据库，提供独立路径规划、实时地图匹配、转向导航、红绿灯倒计时（±1 s 目标）、限速与测速提示、POI 搜索、中文语音导航，PC 核心 + PC/移动端双前端。

- **需求与技术基线**：[Euro Truck Simulator 2 外部智能导航系统-v0.2.md](./Euro%20Truck%20Simulator%202%20外部智能导航系统-v0.2.md)
- **可行性评估**：[ETS2 外部智能导航系统可行性评估.md](./ETS2%20外部智能导航系统可行性评估.md)
- **执行计划与进度**：[PLAN.md](./PLAN.md)
- **当前阶段**：P1（Map Compiler 完整化——执行基线：P1-map-compiler-plan.md；P0 已关门 v0.1.0-p0）

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
├─ tests/              # 自动化测试
├─ docs/               # 格式笔记、决策记录
└─ PLAN.md             # 执行计划与进度
```

## 许可证

**GPL-3.0**（2026-08-09 决定：项目改为 GPL 开源，以允许复用 GPL 生态实现——TruckLib/ETS2LA/TruckSim Maps）。

历史：v0.1 阶段曾定为 MIT；2026-08-09 修订为 GPL-3.0。
