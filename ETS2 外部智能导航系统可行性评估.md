# 《ETS2 外部智能导航系统》可行性评估

> 评估对象：《Euro Truck Simulator 2 外部智能导航系统》需求规格与技术路线（v0.1）
> 评估日期：2026-08
> 评估方法：四路并行联网研究（SCS Telemetry SDK 现状、地图文件解析生态、红绿灯相位同步专项、导航/遥测应用生态），证据分级为"官方文档确认 / 社区证据 / 推断"，事实核查结果与来源附于文内。

---

## 一、总体结论

需求文档的总体判断"项目整体技术可行"**成立**，且其架构分层（Telemetry → 地图解析 → 导航核心 → 多端 UI）与"先验证最高风险模块再开发 UI"的工程顺序判断正确。但研究证据表明，文档对三个问题的评估需要修正：

1. **红绿灯倒计时是文档评估的最薄弱环节，且其风险来源比文档描述的更复杂**。文档把问题界定为"如何从 simulation timestamp 恢复相位"，但实际存在三个文档未覆盖的事实：相位可能锚定于区域加载时刻而非全局时钟、信号灯 interval 是"模拟秒"需按动态时间倍率换算、夜间存在闪烁窗口。文档方案的直接前提（纯计算推导相位）按现有证据**不成立或至少未被证实**。
2. **全自动路由图生成（无人工修复）的风险应列为第一风险**，而非第二。唯一公开实现了"独立路由 + 三端"的 TruckNav 明确承认其路由图经 QGIS 与脚本**大量人工修复**，且未公开构建流水线；TruckSim Maps 自评生成的交叉口几何"远非完美"。文档要求的"地图更新后完全自动生成"，目前无任何公开项目证明可行。
3. **Mod"结构兼容即自动支持"是乐观假设**。整个生态中没有任何解析项目支持 ProMods 等主流 mod（TruckSim Maps、TruckNav 明确不支持；ts-map 需 per-mod offset 配置且仅部分支持），"自动支持"缺乏先例支撑。

其余约四分之三的需求模块（遥测读取、独立路径规划、地图匹配、偏航重规划、POI、限速、TTS、多端架构、性能目标）证据充分，可以按文档方案推进。

---

## 二、对文档关键论断的事实核查

| 文档论断 | 核实结果 | 证据要点 |
|---|---|---|
| "官方文档列出的稳定 SDK 为 1.14" | **属实** | 官方 wiki 信息框与 2022-06-27 修订记录均列 1.14 为 stable；截至 2025–2026 未发现更新版本（wiki 自 2022 年中停滞，游戏已推进至 1.5x，四年未破坏 SDK API）[官方 wiki](https://modding.scssoft.com/wiki/Documentation/Engine/SDK/Telemetry) |
| "可通过 SDK 取得目的地城市/公司及 ID" | **属实**（通道名需以头文件定论） | 官方提供目的地/来源城市与公司的字符串及内部 ID 通道（`job.city.destination(.id)` 系列，社区 Rust 移植列出；另一批下游项目使用 `job.destination.city` 命名，需以 SDK 压缩包头文件原文为准）[通道清单](https://github.com/drysius/scs-telemetry-rs/blob/main/crates/scs-telemetry-sys/README.md) |
| "当前限速可取自 SDK" | **属实**（存在未文档化行为） | `truck.navigation.speed.limit` 为官方通道（m/s），即 Route Advisor 用值；无限速段返回 0、逆行可能为负，未获官方说明，需实测 [头文件镜像](https://github.com/vojtamolda/autodrome/blob/07e8a4a2621d7bd6e420efbc448013d060a15aef/autodrome/simulator/telemetry/scssdk/include/common/scssdk_telemetry_truck_common_channels.h) |
| "simulation timestamp 可用于相位推算" | **部分属实，语义缺口大** | `frame_start` 回调提供 `simulation_time`/`paused_simulation_time`（微秒），官方明确了暂停行为差异，但**未规定零点与重置语义**；社区观测为会话相对计数器，读档/快速旅行/世界重载归零 [官方示例](https://github.com/vojtamolda/autodrome/blob/07e8a4a2621d7bd6e420efbc448013d060a15aef/autodrome/simulator/telemetry/scssdk/examples/telemetry/telemetry.cpp) |
| "TruckSim Maps 已证明可解析官方地图并输出 GeoJSON/PMTiles，支持官方 DLC，解析需数分钟" | **属实**（补正：作者为 truckermudgeon；**不支持第三方 mod**；无独立 semaphore/nav 图层输出） | 项目活跃至 2026-04，支持全部官方 DLC [仓库](https://github.com/truckermudgeon/maps)；parser 输出清单见 [Makefile](https://github.com/softwarehistorysociety/truckermudgeon-maps/blob/main/Makefile)；README 自评交叉口几何"远非完美" |
| "TruckNav 已证明三端组合可行" | **属实**（需附加限制条件） | GPL-3.0 开源，v0.4.4（2026-04，ETS2 1.59），Desktop/Android/Browser + 独立路由 [仓库](https://github.com/Rares-Muntean/TruckNav-Sim)；但 README 明确路由图经 QGIS 大量人工修复、存在断连道路与非法掉头残余，构建流水线未发布 |
| "Prefab 区分 Map Point 与 Navigation Point；Navigation Point 与 Traffic Semaphore ID 绑定" | **属实** | AI 车道路径点带 speed/light/give way/blinker/priority 属性 [ZModeler 指南](https://forum.scssoft.com/viewtopic.php?t=52334)；ts-map 用 prefab AI 车道重建交叉口路网 [Mapping Information](https://github.com/mike-koch/ets2-mobile-route-advisor/wiki/Mapping-Information) |
| "信号灯周期（绿/黄/红/红黄 + cycle 偏移）可从地图资源获得" | **属实**（证据最充分的一环） | `def/world/semaphore_profile.sii` 为普通文本，官方提取器可解包；`interval[]`（绿黄红红黄秒数）、`cycle[]`（相对周期起点偏移）、`sleep_time`、`inherited` 字段语义有论坛编辑实验确认 [t=237363](https://forum.scssoft.com/viewtopic.php?t=237363) [t=322572](https://forum.scssoft.com/viewtopic.php?t=322572) |
| "SDK 无信号灯通道"（文档隐含依赖） | **属实** | 官方 107 个通道无信号灯状态；唯一沾边的是闯红灯罚款事件（事后触发）[通道列表](https://forum.scssoft.com/viewtopic.php?t=240843) |
| "共享内存传输是成熟方案" | **属实** | RenCloud/scs-sdk-plugin 为事实标准（`Local\SCSTelemetry`）；注意共享内存布局约一年一次偏移变化，须校验 `pluginRevision` [V1.11 说明](https://github.com/RenCloud/scs-sdk-plugin/releases/tag/V.1.11) |
| "Rust 适合做 Navigation Core" | **成立**（但地图编译环节无 Rust 生态） | Rust 生态仅覆盖遥测绑定；**SCS 地图二进制解析的成熟实现全部在 TypeScript（TruckSim Maps）与 C#（TruckLib、ts-map）**，无 Rust crate [TruckLib](https://github.com/sk-zk/TruckLib) |

---

## 三、需要修正的三个核心问题

### 3.1 红绿灯倒计时：风险等级正确，但风险内容需重定义

文档第 14–16 节把问题表述为"周期数据 + cycle offset + simulation timestamp → phase(t)"，即**纯计算方案**。研究证据显示该方案存在三个未覆盖的障碍：

**其一，相位锚定方式未证实，且证据指向不利方向。** SCS 论坛对 ETS2 1.50 的讨论中，社区通行解释是：信号灯在距玩家特定距离处流式生成，**生成时处于相同默认状态，动画时长恒定**；玩家反复观察到"以相近速度经过同一路口总是遇到相同状态"，被误认为绿灯波。由此推断相位计时器在灯组随区域加载时从默认状态启动，与 game.time 无全局同步；读档、传送、区域卸载重载均会重置相位 [p1952573](https://forum.scssoft.com/viewtopic.php?p=1952573)。该解释无官方文档证实，但它是目前唯一有证据支持的模型。**若此模型成立，则"从 game.time 推导任意路口当前相位"在数学上不可能**——相位是加载事件的函数，而加载时刻由玩家移动历史决定，无法先验计算。这是决定整个倒计时模块成败的唯一分叉点，文档的 P0-E 实验恰好能判定它，但实验设计需要扩展（见第五节）。

**其二，interval 是"模拟秒"，必须按动态时间倍率换算。** ETS2 时间倍率随区域变化（社区高度一致的观测：城市约 1:3，高速公路约 1:19，无 `g_time_scale` 命令可调，`warp` 会加速），信号灯按游戏内时间计时。这意味着：profile 中"30 秒红灯"在城市是 10 真实秒，在高速是约 1.6 真实秒。倒计时若以真实秒显示（驾驶决策基于物理时间），需要实时读取倍率（官方通道 `game.scaled_time` 提供）并处理倍率跳变；文档的验收标准 |t_shown − t_actual| ≤ 1 s（真实秒）在 1:19 倍率下要求引擎计时精度达到约 ±19 模拟秒的容差——在高速路段该功能本身也几乎失去意义（1.6 秒的红灯窗口）。合理的处理是：**倒计时仅在城市/低倍率区域声明支持，且验收条件按"倍率已知且稳定"的前提重定义**。

**其三，夜间闪烁窗口。** profile 含 `sleep_time_start/end`（社区默认约 23:30–03:00），该时段信号灯切换为夜间闪烁、不按周期循环，倒计时必须显式失效。文档未涉及。

综合结论：**纯计算方案按现有证据不可行；"观测锚定 + 周期外推"的混合方案有条件可行**——在灯组加载后（此时灯组已在视野内）、非夜间、倍率已知的条件下，一次性观测当前相位（视觉识别或内存读取，SDK 无此通道），即可外推该灯组后续转换。生态中最接近的先例是自动驾驶项目 ETS2LA 的 TrafficLightDetection（v1.6.4 起"track the lights"持续跟踪检测到的灯）[ETS2LA](https://github.com/ETS2LA/Euro-Truck-Simulator-2-Lane-Assist)。但需注意：若相位随加载重置，则"记住每个路口相位"不可行，每次接近都需重新观测——不过这与倒计时的使用场景（灯组可见时才显示）基本重合，混合方案仍具工程价值。**任何倒计时实现均无公开先例，本模块的 Go/No-Go 决策必须完全依赖 P0 实验数据。**

### 3.2 全自动路由图生成：无先例，应列为第一工程风险

文档 Risk 2 要求"更新地图后完全自动生成路由图"。现有证据链：

- TruckNav（唯一开源独立路由项目）：路由图由作者经 QGIS + PyQGIS **大规模人工修复**（节点吸附、环岛与交叉口转向限制、单行道、公司园区接入），README 原文称"massive amount of work"，仍残留断连道路与非法掉头，且项目仍标注 Alpha [README](https://github.com/Rares-Muntean/TruckNav-Sim)。
- TruckSim Maps（最活跃的解析器）：自评"生成的道路/prefab GeoJSON 远非完美，许多交叉口形状不正确"。
- 格式无官方规范，随版本漂移：TruckLib 标注支持 1.59–1.60、ts-map 至 1.58、官方提取器自 1.55 起更换为 64 位新版本——维护成本是生态所有项目的共同痛点 [Game Archive Extractor](https://modding.scssoft.com/wiki/Documentation/Tools/Game_Archive_Extractor)。

因此"全自动生成"意味着要解决 TruckNav 用大量人工才解决的问题。文档的 graph validation 清单（dead-end、非法掉头、单向校验、孤立分量、prefab 连通性、环岛、公司入口）方向正确，但应认识到：**验证只能发现错误，不能修复拓扑语义**；环岛分支计数、prefab 内转向限制等语义错误在自动验证下可能表现为"合法但错误"。建议的缓解路径：

1. 以 TruckSim Maps parser（或 ETS2LA/data fork，后者额外导出 prefab AI 导航数据）为语义层基座，不重复造轮子；
2. 建立"对照测试集"：以 TruckNav 已报告的错误类型（断连道路、非法掉头）和 ETS2 已知路网事实为回归基准；
3. 将"V1 全自动 + 零人工"的验收降为"全自动构建 + 自动验证通过 + 已知错误率可接受"，保留人工修复通道（文档第 56 节仓库结构未含修复工具链，建议补充）。

**语言选型需调整**：文档建议 Rust 编写全部组件，但 Rust 生态没有 SCS 地图二进制解析库。务实方案是地图编译器使用 TypeScript（与 TruckSim Maps 同栈，可参考其代码）或 C#（TruckLib），通过 CLI/文件与 Rust Navigation Core 集成——编译是低频重活，运行期性能要求低，语言一致性不应以重复逆向工程为代价。另需注意许可合规：TruckSim Maps 为 GPL v3，直接复用其代码会使整个发行物受 GPL 约束（若本项目闭源则只能参考实现或独立开发）。

### 3.3 Mod"结构兼容即自动支持"：降级为"尽力而为"

文档第 3 节的原则在生态中无先例：TruckSim Maps 与 TruckNav 明确不支持 mod；ts-map 通过 per-mod offset 配置部分支持且随 mod 版本漂移。已知障碍包括：ProMods 等大型 mod 使用**坐标偏移**（地图整体平移/旋转，需 offset 变换，ETS2LA 为此维护配置表）；mod 自带自定义 prefab 模型与自定义 `semaphore_profile` 覆盖文件，且存在 unit 名冲突导致整个 profile 文件失效的已知案例 [p2105050](https://forum.scssoft.com/viewtopic.php?p=2105050)；多 mod 资源覆盖顺序在解析端难以与游戏实际加载顺序完全一致。

建议：V1 验收范围明确限定为"官方基础地图 + 官方 DLC"（文档第 2.1 节范围本身就是这样，但第 3 节原则制造了超出范围的期望）；mod 支持作为实验特性，先支持无 offset 的简单 mod，并保留文档的 Compatible/Partially Compatible/Unsupported 三档标注机制。

---

## 四、分层可行性矩阵

**A 层：证据充分，可直接按文档方案实施**

| 模块 | 依据 |
|---|---|
| Telemetry 桥（位置/朝向/速度/限速/燃油/疲劳/任务目的地） | SDK 1.14 官方通道 + 共享内存事实标准，生态十年验证 |
| 地图资源解包（.scs HashFS） | 官方 scs_extractor / scs_packer |
| 道路/prefab/公司/POI 静态数据提取 | TruckSim Maps 证明（注意与"路由图"的差距） |
| 独立路径规划（A*/多 profile/多路线去重） | TruckNav、TruckSim GPS 双重证明；亚秒级性能目标合理 |
| 地图匹配、偏航检测与重规划 | 常规工程，无特有障碍；高频坐标 + HMM/递归跟踪方案正确 |
| 限速传播（文档 Risk 3） | 数据可得；用 Telemetry 限速做 ground truth 自动验证的思路正确，且该通道覆盖全部官方地图，应作为主要验证手段 |
| 加油/休息/POI/收费站/渡轮规划 | 数据在 companies/pois/ferries 图层中，常规工程 |
| PC + LAN Mobile 架构、MapLibre + PMTiles、TTS、性能目标 | TruckNav/Funbit 证明；共享内存轮询开销极小，<3% CPU 目标现实 |

**B 层：有条件可行，需先验证或降级**

| 模块 | 条件 |
|---|---|
| 红绿灯倒计时 / 红灯减速 / 即将绿灯 / GLOSA | P0 实验判定相位锚定方式；非夜间、倍率已知且低倍率区域；观测锚定或视觉识别补充；无法满足则退化为 STATE_ONLY（文档已设计此回退，正确） |
| 全自动路由图生成 | 无先例；需对照测试集 + 自动验证 + 保留人工修复通道 |
| 测速摄像头识别 | 编码方式未验证，按文档计划列入 P0/P1 调研，不得在验证前承诺覆盖 |
| 高速出口编号 | 文档已正确降级为"依赖 sign parser 覆盖率验证" |
| Mod 自动兼容 | 降级为尽力而为（见 3.3） |

**C 层：文档方案中需要修改的部分**

| 项目 | 修改建议 |
|---|---|
| P0-E 实验范围 | 增加倍率换算、夜间窗口、读档/传送重置三个测试维度（见第五节） |
| 红绿灯验收标准 | 增加适用条件声明（倍率、夜间、会话连续性）；±1 s 仅在满足条件时适用 |
| map-compiler 语言 | Rust 或 TS/C# 混合，避免 Rust 生态空白 |
| 仓库结构 | 增加 graph-fix-toolkit（人工修复通道）与对照测试数据目录 |
| V1 范围 | 红绿灯倒计时从"V1 必须完成"调整为"P0 验证通过才纳入 V1"；Mod 支持明确排除出 V1 验收 |

---

## 五、P0 实验设计修正（对应需求文档第 16、49 节）

需求文档的 P0-E（"Semaphore profile + cycle offset + simulation timestamp → phase(t)"）是正确起点，但需扩展为四个独立实验，其中第一个决定项目生死：

1. **相位锚定判定实验（决定性）**：固定存档、同一路口，在多个不同 game.time 时刻驶近，记录到达时相位与 game.time mod 周期总长的关系。若呈确定性函数 → 全局时钟假说成立，纯计算方案复活，项目风险大幅下降；若与到达/加载事件相关 → 局部计时器假说成立，倒计时必须转向观测锚定方案。该实验成本低（一个路口、一块屏幕录像、SDK 时间戳），结论决定性，应作为 P0 第一项。
2. **倍率换算实验**：在城市（1:3）与高速（1:19）分别实测信号灯真实周期 vs profile interval，验证 `game.scaled_time` 换算关系与倍率跳变行为。
3. **重置事件实验**：读档、快速旅行、远距离驶离后返回，观测相位是否重置。
4. **夜间窗口实验**：验证 sleep_time 窗口内的闪烁行为与边界时刻。

P0 通过标准建议同步修改：只有实验 1 判定为全局时钟（或观测锚定方案精度达标）且实验 2/3/4 行为可建模，才将倒计时纳入 V1；否则按文档既定回退（STATE_ONLY）处理，项目其余模块不受影响——这恰好验证了需求文档"红绿灯失败不影响其余功能"的架构判断。

---

## 六、最终判断

需求文档是一份质量高于平均水准的预研方案：其风险排序方向正确（信号灯第一、自动图生成第二）、回退机制设计合理（COUNTDOWN_SUPPORTED/STATE_ONLY/UNSUPPORTED 分级）、对"不得将静态事件解析表述为实时事件识别"的边界意识正确，开发顺序（先验证后 UI）符合该项目的不确定性结构。

需要修正的核心结论有三条：其一，红绿灯倒计时的关键障碍不是"如何从 timestamp 恢复相位"，而是"相位是否由全局时钟决定"——现有证据指向不利答案，必须由 P0 实验裁定，且实验须扩展倍率、夜间、重置三个维度；其二，全自动路由图生成缺乏任何公开先例，风险应上调至第一位，需以 TruckSim Maps 为基座、以对照测试集保障质量、保留人工修复通道；其三，Mod"结构兼容即自动支持"应明确降级。

在此基础上，项目作为单人或小团队项目具有现实可行性，工作量主体在地图编译与验证体系，而非导航算法本身；建议以"4 周 P0（遥测 + 单城市解析 + 相位判定实验）"作为第一个里程碑，P0 数据将直接决定红绿灯模块与整体投入产出比。

---

## 附：评估依据（研究产出）

- 四路研究简报：`E:\Projects\Pi\ETS2Nav\.pi-subagents\artifacts\outputs\`（e7d748c3：Telemetry SDK；dba4a7c8：地图解析生态；079369d1：红绿灯相位同步；1303cf05：导航应用生态）
- 证据分级约定：官方文档确认（wiki/头文件/官方工具）＞ 社区证据（论坛编辑实验、项目代码、README）＞ 推断；文中均已标注
