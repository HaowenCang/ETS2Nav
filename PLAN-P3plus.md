# 后续计划拆分：独立开发 vs 实机测试配合

> 依据：PLAN.md §4 里程碑（P3–P6）、v0.2 技术基线（§18、§35–42、§56–63、§66）、
> p2-gameplay-test-checklist-2026-08.md（T1–T6）。
> 拆分原则：**凡能用合成数据、回放 trace、离线语料验证的工作归 A（独立开发）；
> 凡必须由真实游戏运行产生输入或验收结果的工作归 B（实机测试配合）**。
> 状态约定与 PLAN.md §3 一致：`⏳ pending` / `🔄 in_progress` / `✅ done`。

---

## §1 拆分总览

| 部分 | 性质 | 内容 | 预计投入 |
|---|---|---|---|
| A1 | 独立开发 | P3 提醒模块核心逻辑（限速链/超速/红灯减速/即将绿灯/GLOSA/TTS） | 主要工作量 |
| A2 | 独立开发 | P4 UI（Browser → Desktop → LAN Mobile），回放驱动开发与验证 | 主要工作量 |
| A3 | 独立开发 | P5 自动化 OD corpus 基建与全欧洲检查 | 中等 |
| A4 | 独立开发 | P6 性能优化（ALT/CH、增量编译）与 synthetic benchmark | 中等 |
| A5 | 独立开发 | B7（TL-03/04/05）实验工具与脚本准备 | 小 |
| B1 | 实机配合 | 插件安装与冒烟（一次性） | 约 10 分钟 |
| B2 | 实机配合 | 单趟综合驾驶采集（覆盖 T1–T6） | 约 20–30 分钟 |
| B3 | 实机配合 | B7 特殊行为实验（Warp/Reset/特殊 profile） | 约 15–30 分钟 |
| B4 | 实机配合 | P3 提醒模块实机验收（限速/超速/GLOSA/测速/语音） | 分项 5–10 分钟 |
| B5 | 实机配合 | P4 移动端 LAN 连接与渲染实测 | 约 10 分钟 |
| B6 | 实机配合 | §63 正式性能验收（P0-D 方法复测，含完整导航负载） | 约 15 分钟 |

A 部分不阻塞用户；B 部分可分批安排在用户方便时，单次驾驶可覆盖多项。

**执行模式（2026-08-11 决策）**：用户确认**暂不进行实机测试**——B 侧（B1–B6）整体延后，A 侧连续推进。关门口径沿用 P2 先例：机器可验证部分完成即关门（tag），实机验收项登记为已知限制；P3 提醒模块按设计默认关闭交付，启用边界待 B2/B4 数据闭合。A3（P5）为唯一可自足出口阶段，可正式关门。B 侧集中一次游戏会话（约 1 小时）统一补齐：B1+B2+B3 → 分析报告 → B4/B5/B6。

---

## §2 A 部分：独立开发项

### A1. P3 Driving Assistant 核心逻辑（v0.2 §35–42、§48–49）

开发内容：

1. **前方限速数据链（§40）**：Map Compiler 侧建立沿 RoutingEdge 的 speed-limit interval（`0–250m: 80` 式分段）；dataset v2 schema 扩展（新增 speed segments 字段）；Rust 侧前方限速热路径查询。P1-09 裁剪项（speed segments/SignMetadata）在此补齐。
2. **提醒决策模块**：当前限速对照（telemetry vs map，§39 diagnostic event 机制已有雏形）、超速提醒（§41 默认阈值 ≤50: +3、>50: +5，可配置）、红灯减速提示（§36 `d_stop = vt_r + v²/2a + d_m` 保守模型）、即将绿灯（§37 条件组合）、GLOSA（§38 速度窗口与限速/加速度求交集，2–5 Hz 更新，输出低精度区间）。
3. **TTS 语音播报（§48–49）**：Windows 离线 TTS 通道 + 播报频率管理（同一类提醒最小间隔、优先级队列），与 UI 事件流解耦。
4. **测速摄像头数据验证（§42，V1 P1 数据验证项目）**：对全欧洲编译产物提取 camera position/controlled direction/speed limit，统计覆盖率；据结果给出 Go/No-Go——可稳定提取则实现"前方 500 m 测速"，否则不实现并记录结论。

验证方式（全部离线）：

- 合成 trace + 回放：用 nav-core-cli replay 驱动事件流，断言提醒触发条件（构造限速变化段、信号状态序列、接近速度组合）；
- 限速链用 Europe dataset 全量构建验证 + 已知路径抽查；
- GLOSA 用合成信号窗口验证输出区间数学正确性（§38 示例 53.6–64.2 → 显示 50–60）；
- camera 验证产出覆盖率报告（docs/validation/p3-camera-coverage-2026-08.md）。

依赖与启用边界：模块可先行开发并以合成数据验证；**实机启用前需 B2 的 T1 数据闭合限速一致率、T3 数据确认信号 runtime 关联**。未闭合前提醒模块以"可配置开关默认关闭"交付，避免未经验证的误报进入实际使用。

### A2. P4 正式 UI（v0.2 §56–61）

开发内容：

1. **Browser Debug UI 升级为正式布局**（§56 高德式信息结构：下一转向卡片、地图、速度/限速、信号与 GLOSA 卡片、剩余距离/路线/设置）。
2. **Desktop（Tauri）**：复用同一 TypeScript + MapLibre GL 前端；pmtiles 地图渲染（§59，坐标转换只在渲染层）。
3. **自动缩放与 Junction View（§57）**：`Z = f(v, D_maneuver, junction complexity)`；手动拖动后暂停 follow 并自动恢复。
4. **PC → 移动端 API 定稿（§60）**：HTTP（metadata/search/route/settings）+ WebSocket（vehicle/route_progress/maneuver/speed_limit/traffic_light/warning/map_state，10–20 Hz）；移动端插值实现 60 FPS truck icon 动画。
5. **LAN 连接（§61）**：Nav Server 默认 localhost + RFC1918 私网监听，二维码（IP/port/token）生成；PWA 移动端。
6. 60 FPS 目标验证：用回放 trace 模拟 20–50 Hz vehicle 流，DevTools/性能面板测渲染帧率。

验证方式（全部离线）：回放 trace 驱动完整 UI 交互（设目的地→导航→偏航→重规划→到达）；合成信号/限速事件流验证卡片展示。

依赖：A1 的提醒事件输出（以合成事件替代即可并行）；**B5 负责真机扫码与移动端实机验收**。

### A3. P5 自动化 OD corpus（v0.2 §18、§66）

1. **已知路线集**：覆盖英国/法国/德国/北欧/巴尔干/意大利/西班牙/东欧/官方最新 DLC 区域，每条含 origin/destination/expected mandatory roads/forbidden maneuvers/ferry-train 使用/环岛出口/公司入口；期望值从当前 Europe dataset 自动提取基准并在编译器修改后 diff 回归。
2. **随机 OD 检查**：数千 OD pair 全图自动化检查——可达性、geometry 连续、不合理掉头、graph jump（§66 框架在 P1-06/P1-07 已有，扩展至全欧洲）。
3. 与 run-p1-tests.bat / run-p2-tests.bat 同构的 run-p5-tests.bat 单命令套件。

验证方式（全部离线）：Europe dataset + 回归套件；**P5 的出口验收即套件全绿**。

### A4. P6 性能优化（v0.2 §62）

1. **ALT/CH 分层路由**：评估在当前基线（路线 p99 0.39 ms，已超出 §62 典型 <1 s 目标 1000×）上的收益；若 benchmark 显示必要性则实现，否则登记为"目标已达成，不做"（§62 是验收标准而非无条件工作项）。
2. **增量编译（§9 地图更新检测）**：DLC/游戏版本变更检测 + dataset 增量重建；fingerprint 机制 P1-02 已有基础。
3. **§63 正式验收方法固化**：以 B6 实测数据为最终依据；synthetic benchmark 只做开发期回归。

### A5. B7 实验工具准备（v0.2 §64 TL-03/04/05）

实验采集脚本与判定流程就绪（signal-lab 扩展：warp 标记、读档标记、特殊 profile 路口记录）；**实际数据采集归 B3**。

---

## §3 B 部分：实机测试配合项

### B1. 插件安装与冒烟（一次性，约 10 分钟）

操作：复制 `scs-nav-bridge.dll`、`semaphore-bridge.dll` 至
`E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2\bin\win_x64\plugins\`；
启动游戏；运行 `nav-core-cli live` 确认遥测帧持续输出。

### B2. 单趟综合驾驶采集（约 20–30 分钟，覆盖 T1–T6）

按 p2-gameplay-test-checklist-2026-08.md §三 路线：**城市出发（T1 城市段 + T3 路口）→ 高速（T1 高速段）→ 英国（T2 环岛）→ 回程偏航一次（T4）**。建议附加德国不限速段（T1 第 4 项）。

- 用户操作：按路线驾驶一趟，期间设置一次导航目的地并故意偏航一次；FPS 对比轮（T5）需插件卸载 vs 加载各驾驶一轮（或导航开 vs 关）。
- 数据交付：trace 文件路径（默认 Temp 目录，可指定）；T5 的 FPS 记录。
- 分析侧（A 侧独立完成）：T1 限速对照表 + 报告；T2 环岛方向判定；T3 信号关联置信度；T4 状态机全程检查；T6 matcher 权重校准建议。
- 产出：docs/validation/p3-gameplay-2026-08.md 汇总报告，关闭 P2 已知限制。

### B3. B7 特殊行为实验（约 15–30 分钟）

在信号路口分别执行：游戏内传送（warp）后观察相位、读档（reset）后观察相位、经过特殊 profile 路口（B7 实验内容）。数据经 A5 工具采集，分析侧判定行为模型。

### B4. P3 提醒模块实机验收（A1 交付后，分项 5–10 分钟）

限速/超速提醒与实际 HUD 一致（含 fallback 合理性）；GLOSA 输出区间可用性；测速摄像头（若实现）误报率抽查；TTS 播报时机与频率体验。

### B5. P4 移动端实测（A2 交付后，约 10 分钟）

手机与 PC 同一局域网：扫码连接 Nav Server；导航页面 60 FPS 渲染体验；断线重连行为。

### B6. §63 正式性能验收（约 15 分钟）

P0-D 方法（PresentMon + analyze.sh 管道已就绪）复测：ETS2 only vs ETS2 + Nav Core（完整导航负载：路线 + 提醒 + 移动端渲染），验收 avg FPS 差异 ≤1–2%、1% low 回退 ≤2%。

---

## §4 依赖关系与执行顺序

```
A1 限速链+提醒模块 ──┐
                     ├──▶ B4 提醒实机验收 ──┐
A5 B7 工具 ──────────┴──▶ B3 B7 实验 ──────┤
                                            ├──▶ P3 关门
B1 冒烟 ──▶ B2 综合采集（T1–T6）───────────┘
（B2 数据同时闭合：T1 限速一致率 → A1 启用边界；T3 → 信号提醒启用；T6 → matcher 权重）

A2 UI（可与 A1 并行，合成事件驱动）──▶ B5 移动端实测 ──▶ P4 推进
A3 OD corpus（依赖 Europe dataset，已有）──▶ P5 出口
A4 性能优化 ──▶ B6 正式验收 ──▶ P6 出口
```

关键点：

- **A1 与 B2 无相互阻塞**：A1 以合成数据先行开发；B2 数据到达后闭合启用边界（限速一致率、信号 runtime、matcher 校准），再进入 B4。
- **B2 是 B 部分的枢纽**：一次驾驶覆盖 T1–T6 多数项，建议优先安排。
- B3 与 B2 可在同一次游戏会话中顺带执行（经过路口时先做 warp/读档操作）。
- P3/P4/P5/P6 四个阶段中，P3 需要 B 部分数据才能关门（实机验收性质），P4 仅 B5 一项依赖移动端真机，P5 全离线，P6 依赖 B6 最终验收。

---

## §5 状态跟踪表

| ID | 内容 | 前置 | 状态 | 验证/产出 |
|---|---|---|---|---|
| A1 | P3 提醒模块核心逻辑 | — | ✅（2026-08-11，tag v0.4.0-p3） | speed/reminder/speak 三模块 + 38 测试；camera No-Go；run-p3-tests.bat ALL PASS；启用边界待 B2、实机验收待 B4 |
| A2 | P4 UI（Browser/Desktop/LAN Mobile） | A1 事件接口（可先以合成事件） | ✅（2026-08-12） | nav-server（HTTP+WS 零依赖）+ 正式 UI（§56/§57/§59）+ API 定稿（§60）+ LAN/二维码/PWA（§61）+ Desktop（Tauri 2）；批 1 审计 MAJOR 11 修复、复审 0/0；verify-ui-chain.py 6 项 ALL PASS；**P4 关门**（p4-closeout-2026-08.md）；60 FPS 与移动端待 B5 |
| A3 | P5 OD corpus 与全欧洲检查 | — | ✅（2026-08-12） | od-corpus 四子命令 + 九区域 36 对基准 + 2000 对随机 OD + run-p5-tests.bat ALL PASS；批 1 审计（a3-correctness/a3-data-integrity）MAJOR 5 修复、复审 0/0；**并完成登记图缺陷的根因排查与修复**（UK 孤立/ferry 悬空为编译器缺陷；主分量 75.78%→79.35%、transit 端点孤立 258→0、uturns 1→0；残余断簇判定为源数据）；**P5 关门**（p5-closeout + p5-graph-defects + p5-graph-defects-verification） |
| A4 | P6 性能优化（ALT/CH 评估、增量编译） | — | ✅（2026-08-12） | ALT/CH 登记「目标已达成，不做」（§62 口径裕度 2,600×/5,100×）；增量指纹 `map-inspector --check-fingerprint`（§9 覆盖 4/9）；批 2 审计（a4-perf-truth/a4-conclusion）MAJOR 7 修复、复审 0/0；**P6 关门**（p6-closeout-2026-08.md）；§63 正式验收待 B6 |
| A5 | B7 工具准备 | — | ✅（2026-08-12） | signal-lab 扩展（R=疑似重置/快速旅行、P=特殊 profile 标记）+ SignalLabAnalyze（TL-03 warp / TL-04 reset 检测）；批 2 审计（a5-exec BLOCKER 1 / a5-spec MAJOR 4）修复、复审 0/0；数据采集待 B3 |
| B1 | 插件安装与冒烟 | — | ⏳ 延后（2026-08-11） | nav-core-cli live 输出持续帧 |
| B2 | 单趟综合驾驶采集（T1–T6） | B1 | ⏳ 延后（2026-08-11） | trace + FPS 记录；关闭 P2 已知限制 |
| B3 | B7 特殊行为实验 | A5 | ⏳ 延后（2026-08-11） | warp/reset/特殊 profile 行为判定 |
| B4 | P3 提醒实机验收 | A1、B2 | ⏳ 延后（2026-08-11） | 一致率/误报/播报体验记录 |
| B5 | P4 移动端实测 | A2 | ⏳ 延后（2026-08-11） | 扫码连接 + 60 FPS + 断线重连 |
| B6 | §63 正式性能验收 | A2/A4 完成态 | ⏳ 延后（2026-08-11） | PresentMon 对比数据达标 |

**A 侧状态（2026-08-12）**：A1~A5 全部完成并各自关门（报告见各阶段 closeout）。**剩余工作仅 B 侧（B1~B6）**，依赖一次约 1 小时的游戏会话。

---

## §6 建议节奏（2026-08-11 修订：实机延后模式）

1. **即日起（A 侧连续推进）**：A1 限速链与提醒模块 → A2 UI（并行）→ A3 OD corpus（穿插，可完整关门）→ A4/A5。
2. **P3/P4 关门口径（沿用 P2 先例）**：机器可验证部分完成即关门（tag），实机项登记已知限制；提醒模块默认关闭交付，启用边界待 B2/B4 数据闭合。
3. **A3（P5）优先穿插**：全离线、出口自足，是实机延后期唯一可正式关门的阶段，避免等待期空转。
4. **统一实测会话（用户方便时一次约 1 小时）**：B1 冒烟 + B2 综合采集（含 T5 FPS 两轮）+ B3 顺带执行 → 分析侧报告（关闭 P2 已知限制、闭合 A1 启用边界）→ B4 → B5 → B6。
5. **B 侧完成后**：统一更新各阶段关门报告、本表与 PLAN.md §1。
