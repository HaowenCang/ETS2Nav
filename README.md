# ETS2Nav — Euro Truck Simulator 2 外部智能导航系统

独立于游戏原生 Route Advisor 的外部智能导航系统：自行解析 ETS2 地图资源建立导航数据库，提供独立路径规划、实时地图匹配、转向导航、红绿灯倒计时（±1 s 目标）、限速与测速提示、POI 搜索、中文语音导航，PC 核心 + PC/移动端双前端。

- **需求与技术基线**：[Euro Truck Simulator 2 外部智能导航系统-v0.2.md](./Euro%20Truck%20Simulator%202%20外部智能导航系统-v0.2.md)
- **可行性评估**：[ETS2 外部智能导航系统可行性评估.md](./ETS2%20外部智能导航系统可行性评估.md)
- **执行计划与进度**：[PLAN.md](./PLAN.md)
- **当前阶段**：**P0~P6 A 侧全部关门（2026-08-12，tag `v0.6.0-p4p5p6` @ `7900bbc`；2026-09-11 完成 GitHub 与文档同步复核 + B 侧采集链缺陷修复）**——P3 关门 tag v0.4.0-p3；A2~A5（P4 UI / P5 OD corpus / P6 性能评估 / B7 工具）tag v0.5.0；此后完成 **P5 图缺陷根因排查与修复**（UK 孤立与 ferry 悬空为真实编译器缺陷——主分量 75.78%→79.35%、transit 端点孤立 258→0、od-check uturns 1→0）、**离线加固四项**（UI 链断言不稳定、§9 mod 指纹漏报、§57 规范违背、UI 事件分发）与 **B 侧采集链三项缺陷修复**（`live` 无法启动 / 不录制 trace / trace 无信号灯数据），run-p1/p2/p3/p5 四套件 ALL PASS。存量门复核：cargo 104 测试 + dotnet 80 测试（8 项目）全绿。**唯一剩余工作为 B 侧实机测试**（runbook 已就绪）。
- **关门报告**：P4 / P5 / P6 见 [docs/validation/](./docs/validation/)（`p4-closeout` / `p5-closeout` / `p6-closeout-2026-08.md`）；P5 修复根因与验证见 `p5-graph-defects-2026-08.md` / `p5-graph-defects-verification-2026-08.md`；离线加固见 `p4-p6-hardening-2026-08.md`
- **B 侧会话操作手册**：[b-session-runbook-2026-08.md](./docs/validation/b-session-runbook-2026-08.md)（含 mod 激活集核对）
- **数据集**：[Release `dataset-europe-v5`](https://github.com/HaowenCang/ETS2Nav/releases/tag/dataset-europe-v5)（166 MB 归档）——**无需拥有游戏**即可运行导航核心与 UI 联调。本地重建（需游戏，约 4 分钟）见 `docs/validation/p5-closeout-2026-08.md` §6。注意数据集受 `.gitignore` 约束不入库。

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

## 验证与复现入口

前置：数据集不入库，须先构建 `data/europe-v5`（约 4 分钟，需已安装 ETS2），或从 [Release `dataset-europe-v5`](https://github.com/HaowenCang/ETS2Nav/releases/tag/dataset-europe-v5) 取得后解压至 `data/europe-v5`。构建命令见 `docs/validation/p5-closeout-2026-08.md` §6。

```bat
:: run-p1 不自行设置 ETS2_INSTALL，需先导出（run-p2/p3/p5 内部已设，会链式调用 run-p1）
set ETS2_INSTALL=E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2

run-p1-tests.bat        :: P1 Regression Suite（unit / Berlin+Germany gate / determinism / Rust reader / Europe scale）
run-p2-tests.bat        :: P2 Regression Suite（7 步，含自生成 trace）
run-p3-tests.bat        :: P3 提醒模块（链内 P1 → P2 → P3）
run-p5-tests.bat        :: P5 OD corpus（基线生成 + OD 回归 + OD 检查）

cd nav-core && cargo fmt --check && cargo clippy --all-targets && cargo test
dotnet test map-compiler/MapCompiler.sln
```

结果记录（2026-09-11，`main = bdbd7ea`）：四套件全部 ALL PASS（`BAT_EXIT=0` 复核）——`run-p1` 6 步、`run-p2` 7 步、`run-p3` 4 步、`run-p5` 4 步逐步 PASS；cargo fmt PASS / clippy **0 告警** / **104 测试通过**；dotnet **80 测试通过**（8 项目）。逐项实跑输出、GitHub 发布复核与本次修复的三项采集链缺陷见 `docs/validation/p4-p6-hardening-2026-08.md` §9/§9.1/§9.2。

## 目录结构（P0 精简版，完整版见 v0.2 §72）

```
ets2-nav/
├─ telemetry-plugin/    # C++ DLL：SDK 1.14 + 共享内存（scs-nav-bridge / semaphore-bridge）
├─ map-compiler/        # 地图解析与编译（C#，独立实现）→ routing/junction graph、map.db、search.db、pmtiles
│  └─ tests/            # 8 个测试项目，80 测试
├─ nav-core/            # Rust 导航核心：crates/（dataset/graph/spatial/matcher/router/telemetry）+ tools/（nav-core-cli、od-corpus）
├─ tools/               # 工具：map-inspector / graph-debugger / telemetry-dump / signal-lab / speed-validator / ets2nav-web …
├─ desktop/             # Tauri 2 桌面端
├─ docs/                # 格式笔记（format-notes）、决策记录（decisions）、验证报告（validation）
├─ data/europe-v5/      # 规范数据集（不入库；重建或从 Release 获取）
└─ PLAN.md              # 执行计划与进度
```

## 许可证

**GPL-3.0**（2026-08-09 决定：项目改为 GPL 开源，以允许复用 GPL 生态实现——TruckLib/ETS2LA/TruckSim Maps）。

历史：v0.1 阶段曾定为 MIT；2026-08-09 修订为 GPL-3.0。
