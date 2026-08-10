# ETS2Nav 执行计划与进度管理

> 依据：《Euro Truck Simulator 2 外部智能导航系统-v0.2.md》（技术基线）
> 本文件是会话接续的唯一入口：任何会话开始先读本文件的「§1 当前状态」与「§3 任务状态表」，结束时更新之。

---

## §1 当前状态

**最后更新**：2026-08-10（P2 收官审查中——18/19 工作包完成，P2-19 性能基准达标）

- [x] **P0 阶段**（✅ 门评审通过 2026-08-10，tag v0.1.0-p0；A6/A8 工具并入 P1-01）
- [x] **P1 Map Compiler**（✅ 关门 2026-08-10，tag v0.2.0-p1；15 工作包 P1-00~P1-14 完成 + 4 子代理收官评审修复 + Regression Suite ALL PASS；执行基线 = P1-map-compiler-plan.md）
- [ ] P2 Navigation Core（🔄 收官审查 2026-08-10：执行基线 = P2-navigation-core-plan.md；19 工作包 P2-00~P2-19，**18/19 完成**——P2-19 性能基准达标：加载 436ms/内存 217MB/路线 p99 0.38ms/匹配 p99 0.010ms；4 子代理审查进行中；关门 tag v0.3.0-p2）
- [ ] P3 Driving Assistant
- [ ] P4 正式 UI
- [ ] P5 全欧洲测试
- [ ] P6 性能优化与发布

**当前状态**：**P2 收官审查中（2026-08-10）**——18/19 工作包完成（P2-00~P2-18 全合并 main；P2-19 性能基准达标：Europe v4 全量加载 436ms/内存 217MB/路线 p99 0.38ms/匹配 p99 0.010ms，全部远超 §139-140 目标）；4 子代理收官审查进行中（实现正确性/计划符合性/文档一致性/性能边界），BLOCKER/MAJOR 修复后关门 tag v0.3.0-p2。游戏内实测类验证（UK 环岛方向/Speed Gate/信号 runtime/驾驶 corpus）明确排除并记录为 P2 已知限制。**P1 已关门**（tag v0.2.0-p1）；**P0-D 验收通过**（avg -1.3% / 1%low +0.7%）；**P0-B 数据源重大定案**：ETS2LA 插件共存激活信号灯数组（隔离测试定案：仅 ets2la_plugin.dll 单文件激活，无主程序依赖，方案 C：先共存推进后评估逆向）。

**已建立**：执行计划（本文件）、本地 git 仓库、.gitignore、GitHub 仓库（HaowenCang/ETS2Nav private）。

---

## §2 待确认事项（用户决策记录）

| # | 事项 | 状态 | 决策 | 影响 |
|---|---|---|---|---|
| D1 | GitHub 远程仓库：名称、可见性 | ✅ 已确认 | `ETS2Nav`，**private** | 已创建并推送 |
| D2 | 项目许可证 | ✅ 已确认（2026-08-09 修订） | **GPL-3.0**（原 MIT；为复用 GPL 生态实现而改） | LICENSE 已更新；v0.2 §12 的"不复制 GPL 实现"约束解除，可复用 TruckLib/ETS2LA/TruckSim Maps |
| D3 | ETS2 安装路径 | ✅ 已确认 | `E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2` | P0-A 可用真实游戏文件 |
| D4 | P0 内部优先级 | ✅ 已确认 | 按建议：骨架 + P0-A 先行，P0-B 静态部分（B1–B2）并行 | 任务排程生效 |
| D5 | 信号灯数组激活依赖 | ✅ 已确认（2026-08-10） | **方案 C：ETS2LA 共存推进**（plugins 含 ets2la_plugin.dll 单文件激活），逆向激活评估延后 | 产品运行时依赖 ets2la_plugin.dll（闭源）；P0-B 验收按共存状态记录 |

---

## §3 任务状态表

状态约定：`⏳ pending` / `🔄 in_progress` / `🚫 blocked`（附原因）/ `✅ done`（附日期与验证）。

### P0-A 路由图自动生成（最高工程风险，v0.2 §66）

| ID | 任务 | 完成条件 | 状态 |
|---|---|---|---|
| A1 | 下载官方 SDK 1.14 与 scs_extractor（官方 wiki） | 压缩包入库（vendor/） | ✅ 已入库 vendor/scs_sdk_1_14（含官方头文件/示例）、vendor/scs_extractor_1_55.zip + 解包 vendor/scs_extractor/scs_extractor.exe |
| A2 | 解包 base.scs，研读 `def/world/semaphore_profile.sii`、def 资源结构 | 形成格式笔记（docs/format-notes/） | ✅ def.scs 已解包（66928 条目）+ base_map 全欧洲 1358 sector 解析零错误（P1-03 实证）；笔记 docs/format-notes/（hashfs.md 等） |
| A3 | Map Compiler 骨架（语言按 v0.2 §12：C# 优先；独立实现，不复制 GPL 代码） | 可解包 .scs HashFS、列出 archive 内容 | ✅ 全部完成：HashFS v1/v2 读取器 + ScsSector 解析器（17 种 item 类型 + 节点表）；**282 sector 与 TruckLib 逐项对照零差异**（174735 items / 248410 nodes，24 单测全过） |
| A4 | 单城市解析：sector → prefab → navigation path | 从代表性城市（含十字口/环岛/高速出入口/公司/加油站）提取结构化数据 | ✅ 柏林区域 16 sector 加载（中心 (12725,-9846)，覆盖 sec+0002/+0003-0002/-0003 等） |
| A5 | 有向图构建（Routing + Junction Graph，v0.2 §13–15） | 单城市图可查询 | ✅ ScsGraph：全局节点表 + 跨 sector 连接 + prefab 全连接近似（双向边）；柏林核心城区：道路节点 2426/主分量 87%，+缓冲 16 sector：道路节点 6194/主分量 80% |
| A6 | Graph Validation 基础（v0.2 §17 结构/方向检测） | 检测器可运行并输出报告 | ✅ 并入 P1-01（ScsValidation 拆分正式化） |
| A7 | 100–500 随机 OD 测试（v0.2 §66） | 断路/非法掉头/逆行报告 | ✅ 框架已跑通：主分量内 200/200 OD 100% 可达（随机种子固定）；边界小分量已定位为截断效应；正式化并入 P1-01（ScsValidation）+ P1-06（Berlin Gate） |
| A8 | map-inspector + graph-debugger 工具（v0.2 §74） | 可查看节点/边并点击 A/B 出路线 | ✅ 并入 P1-01（CLI + MapLibre Web + Dijkstra） |
| **门** | **无需 QGIS 人工编辑，单城市 routing graph 基本正确（v0.2 §75）** | — | ✅ 2026-08-10 P0 门评审通过（tag v0.1.0-p0）；柏林核心城区主分量 87%，缓冲区 80% |

### P0-B 红绿灯 ±1 s（最高功能可行性风险，v0.2 §64–65）

| ID | 任务 | 完成条件 | 状态 |
|---|---|---|---|
| B1 | 核对 SDK 头文件通道名（job destination 等，v0.2 §5） | 通道清单定稿（docs/format-notes/telemetry-channels.md） | ✅ 定稿：目的地数据在官方 **configs**（destination.city(.id)/company(.id)），非 channels；官方事件 player.fined/tollgate/ferry/train |
| B2 | semaphore_profile 解析器 + 语义笔记（interval/cycle/id_map/inherited/sleep_time） | 解析器单元测试通过 | ✅ SII 解析器 + profile 模型 + 继承链（11 单测）；真实文件全量验证 278 profile |
| B3 | telemetry-plugin DLL（C++，SDK 1.14，共享内存 + sequence counter，v0.2 §6–7） | telemetry-dump 能显示 position/heading/speed/timestamps/限速/job | ✅ 游戏内验证通过（2026-08-09）：全字段正常、无崩溃；实证城市 scale=3.0、限速 0=无限速；job 字段待接任务补验（见 docs/validation/b3-telemetry-2026-08-09.md） |
| B4 | signal-lab 工具（记录 signal 事件 + 各 clock + 误差分析） | 可回放实验数据 | ✅ signal-lab（高频采样+按键标记+CSV）+ signal-analyze（TL-01/02 判定）+ 模拟验证 PASS；**待真实游戏数据采集**（用户配合） |
| B5 | 实验 TL-01 Clock Domain（v0.2 §29、§64） | 判定 semaphore interval 所属时钟域 | ✅ **结论：simulation_time 驱动，interval 秒=真实秒（1:1）**——7 间隔全部倍率 1.000±0.7%；周期 59.5s≈60s profile；排除 game.time（见 docs/validation/tl01-clock-domain-2026-08-09.md） |
| B6 | 实验 TL-02 Phase Anchor（H1 全局 vs H2 局部，v0.2 §30） | 判定相位锚定模型 | ✅ **H2 确认**：sim 连续窗口内驶离返回后相位跳变（5.9→27.5s mod 周期）；纯计算倒计时不可行，需观测锚定+外推（见 docs/validation/tl02-phase-anchor-2026-08-09.md） |
| B7 | 实验 TL-03 Warp / TL-04 Reset / TL-05 Special Profiles（v0.2 §64） | 行为建模 | ⏳（v0.3 与逆向激活评估一并规划） |
| **门** | Go/No-Go：Case A/B（|e|≤1 s，v0.2 §65）→ 倒计时入 V1；Case C → STATE_ONLY；Case D → UNAVAILABLE | — | ✅ **Go（Case B）**：TL-01 时钟域确定（1:1 真实秒）、TL-02 H2 加载锚定确认、同窗口外推实测最大误差 0.483s ≤1s 验收（见 docs/validation/p0b-traffic-light-conclusions-2026-08-09.md）；倒计时方案：观测锚定+段长学习+外推，未锚定期间 STATE_ONLY |
| B8 | **自研精确数据源 semaphore-bridge**（v0.2 §64 超越项） | 游戏内内存读取，共享内存 Local\ETS2NavSemaphore | ✅ **v8 定稿**：48B/灯布局反查定案（pos+cx/cy+quat+type+time+state+id）；SEH 兜底+协作取消+越界修复；**隔离测试定案：需 ets2la_plugin.dll 共存激活数组**（见 docs/validation/semaphore-bridge-2026-08-10.md）；退出游戏不崩溃已由 C1 实测闭环（2026-08-10） |

### P0-C Telemetry 稳定性（v0.2 §75）

| ID | 任务 | 完成条件 | 状态 |
|---|---|---|---|
| C1 | 数小时连续运行：不崩溃、数据连续、pause/load 正确 | 实测报告 | ✅ 用户实测通过（2026-08-10）：挂机数小时无崩溃、退出游戏无崩溃（v8） |

### P0-D 性能基线（v0.2 §63、§75）

| ID | 任务 | 完成条件 | 状态 |
|---|---|---|---|
| D1 | ETS2 only vs ETS2+Core 基准脚本（FPS/1% low/frametime） | 对比数据 | ✅ **验收通过**（2026-08-10）：avg 150.2→148.2（-1.3% ≤1-2%）、1% low 94.2→94.9（+0.7% 无回退 ≤2%）；PresentMon 2.5.1 + analyze.sh 可靠管道（mawk 无 asort，已用 awk 排序替代） |

### P0 交叉任务

| ID | 任务 | 完成条件 | 状态 |
|---|---|---|---|
| X1 | 仓库骨架：目录结构（v0.2 §72 精简到 P0）、.gitignore、README | 初始提交完成 | ✅ 已完成（含 GitHub 推送） |
| X2 | 决策记录文件（docs/decisions/，记录每次关键选择） | 随项目维护 | ✅ ADR-001~007 已建（2026-08-10） |

---

## §4 里程碑与阶段映射（v0.2 §73）

| 阶段 | 内容 | 入口条件 | 出口验收 |
|---|---|---|---|
| P0 | P0-A 路由图 + P0-B 红绿灯并行（+C/D） | 本计划确认、游戏路径可得 | v0.2 §75 四项：Map 无人工修复可用、Signal 时钟域/相位锚定明确（countdown 则 ≤1 s）、Telemetry 数小时稳定、性能无显著影响 |
| P1 | 完整 Map Compiler（base map + 官方 DLC + graph + POI + sign + semaphore + vector tile，v0.2 §8–19） | P0 门通过 | 全欧洲编译产物 + 回归测试集（v0.2 §18） |
| P2 | Navigation Core（Map Matching、A*、profile、alternatives、rerouting、maneuvers，v0.2 §20–26、§43–44） | P1 产物 | 亚秒级路线、偏航重规划 ≈1 s |
| P3 | Driving Assistant（限速/超速/测速/红绿灯/GLOSA，v0.2 §36–42） | P2 | 各提醒模块验收 |
| P4 | 正式 UI（Browser → Desktop → LAN Mobile → Android/iOS，v0.2 §56–61） | P2/P3 | 高德式信息结构 + 60 FPS 目标 |
| P5 | 全欧洲官方地图测试（automated OD corpus，v0.2 §18、§66） | P1/P4 产物 | 数千 OD 自动检查通过 |
| P6 | 性能优化（ALT/CH、增量编译等）与正式发布（v0.2 §62–63） | P5 | benchmark 目标达成 |

---

## §4.1 P1 工作包状态（执行基线：P1-map-compiler-plan.md）

| ID | 任务 | 完成条件 | 状态 |
|---|---|---|---|
| P1-00 | Baseline Freeze（README/PLAN/ADR/决策记录） | P1 baseline documented | ✅ 2026-08-10：ADR-001~007 建齐 + P1-map-compiler-plan.md 执行基线定稿 |
| P1-01 | Validation & Debug Tooling（GraphValidator 拆分 + severity + diagnostics + map-inspector + graph-debugger） | Berlin P0 graph 可完整可视化并显示 validation error | ✅ 工具全部落地（2026-08-10）：ScsValidation（6 测试）+ map-inspector（--defs/--prefab）+ graph-debugger；Berlin 可视化验收并入 P1-06 Gate |
| P1-02 | Resource Resolver（IScsResourceProvider + HashFs/Directory/Overlay + DLC 检测 + fingerprint） | 所有上层模块仅经 virtual resource API 读取 | ✅ 2026-08-10：Resolver 层（8 测试）+ 评审修复（Dispose/ResolveSource/指纹 UTC/路径穿越/探测警告）+ **上层迁移完成**（SectorFile 流重载 + DefinitionLoader/PrefabResolver/SemanticMapBuilder 全经 IScsResourceProvider） |
| P1-03 | Definition Layer（SII corpus + include + road look + country/city/company/semaphore/ferry/train + sign 基础） | Berlin 所需定义全部解析为 typed model | ✅ 2026-08-10：SiiParser corpus 增强（失败率 6.4%→0.3%，余量全在 vehicle/climate 非地图语义）+ DefinitionResolver 诊断（FailedFiles/traffic_rule 模型/去重）+ GpsAvoid 读 road_flags（TruckLib rflag4 bit4）；61 测试（当时快照，收官时 65）；全欧洲 1358 sector 零错误；笔记 road-look.md |
| P1-04 | Prefab Navigation Parser（descriptor/connector/curves/navigation lanes/movement/signal ID） | semantic corpus 全部典型 prefab 恢复合法 movement | ✅ 2026-08-10：ScsPrefab（PpdReader v0x19 对照 TruckLib.Models + PrefabResolver token→PPD 映射 + PrefabMovements movement 恢复含转向分类/信号灯绑定 + 字符集数组化/哈希 token 容错）；Berlin 220/220 prefab、928 movements（收官评审串联展开修复前快照）；4 测试；笔记 prefab-descriptor.md |
| P1-05 | Semantic Graph（SemanticMap + RoadSegment + JunctionMovement + RoutingGraphBuilder + JunctionGraphBuilder；删除双向/全连接近似） | Berlin 正式 semantic graph 可生成 | ✅ Berlin 语义图 6207 边（Road 3701 + Movement 2506，rail 排除修复后）/最大分量 2965；方向翻转实验 94%→2.4% 验证 lanes 方向假设；rail 排除（评审修复后 2205 roads） |
| P1-06 | **Berlin Semantic Gate**（独立 Gate：≥500 deterministic random OD，0 fatal / 0 已知非法 movement / 0 逆行） | Gate 通过 | ✅ 0 fatal/0 非法/0 逆行；核心网 OD 95.4%（rail 排除修复后口径；全图 64%——边界断头为数据范围限制）；死端为 sector 边界断头（不阻断）；见 p1-06 + p1-closing-review-fixes 报告 |
| P1-07 | Germany Scale Test（完整 Germany build + validation 无 blocker + random OD ≥500） | 通过 | ✅ 2026-08-10：Germany 39 sector（11,551 roads/3,323 junctions/485 prefab 种）核心网 OD 94.6%（修复后口径）；Europe 全量构建 2.3s 无 scale 退化；报告 p1-07 |
| P1-08 | POI / Search（city/company/garage/repair/fuel/rest/ferry/train/toll/border + search.db） | POI 均有效 access 或明确非 routing POI | ✅ 2026-08-10：PoiExtractor 9 类型 + PPD SpawnPoint（rest 数据源）+ search.db FTS5；Berlin 79/Germany 375 POI 全 routing access；报告 p1-08 |
| P1-09 | Road Rules / Signs / Speed（speed model + sign parser + speed segments + telemetry validation + camera 调研） | 代表路线 map speed 与 telemetry 高一致率 | ✅（按裁剪口径）2026-08-10：SpeedModel 完成（country×speed_class×城市 flag，三态 -1 未知/0 无限速/数值；城市 bbox 判定）+ speed-validator 工具就绪（共享内存布局核对正确）；**完成条件裁剪**：telemetry 实测一致率延后 P2（需游戏内运行）；speed segments/SignMetadata 未交付（P2）；sign/camera 调研见 format-notes |
| P1-10 | Semaphore Binding（JunctionMovement ↔ SemaphoreProfile/ID） | P0 测试路口均确定 signal group | ✅ 2026-08-10：1.60 实测定案（灯配置全在 PPD；signal group = PPD SemaphoreId）；Berlin 1,387 带灯 movement 100% 确定（串联展开修复后）；报告 p1-10 |
| P1-11 | Dataset Writer（manifest/map.db/routing.graph/junction.graph/search.db/diagnostics.json + Rust smoke reader） | Dataset 可脱离 C# 独立读取 | ✅ 2026-08-10：紧凑二进制（magic/version/endianness）+ SQLite FTS5 + Rust 零依赖独立读取 PASS（Berlin/Germany 全校验）；报告 p1-11 |
| P1-12 | Vector Tiles（map.pmtiles：road/city/POI/developer layers） | graph-debugger/MapLibre 可直接加载 | ✅ 2026-08-10：零依赖 MVT + PMTiles v3（Berlin 121KB/Germany 538KB）；graph-debugger 集成；报告 p1-12 |
| P1-13 | Europe Build（base + 全部官方 DLC；0 fatal / 0 未解析 parser error） | 完成 | ✅ 2026-08-10：全欧洲 679 sector（1358 base+aux）完整 dataset（143,677 roads/426,907 边，rail 排除后 v2 口径）；0 fatal + 0 parser error（2,432 prefab 全加载）；报告 p1-13 |
| P1-14 | Regression Suite（parser/semantic/route corpus + random OD + determinism + reader + scale） | 一条命令跑完整 P1 测试套件 | ✅ run-p1-tests.bat 6/6 PASS（65 测试/Berlin+Germany Gate/determinism 4 产物/Rust reader/Europe scale）；gate 退出码已传播 |

P1 Gates（G1~G13）见 P1-map-compiler-plan.md §126–§138；Exit Criteria §139。

### P1 风险跟踪（§118–124）

| ID | 风险 | 等级 | 缓解 | 状态 |
|---|---|---|---|---|
| R1 | Prefab Navigation Semantics | Critical | semantic corpus + oracle 对照 + graph-debugger + 逐 family 扩展 | 🔄 P1-04 完成（movement 恢复工具链就绪）；语义合法性验证待 P1-05/P1-06 |
| R2 | Road Direction Semantics | Critical | road look + node direction + telemetry traces + fixtures | 🔄 P1-03 完成（Road 字段对照 TruckLib 定稿 + road-look.md 勘误）；graph 语义验证待 P1-05 |
| R3 | Full Europe Format Diversity | High | Berlin → Germany → Europe 分阶段 | ✅ P1-07/P1-13 完成（全欧洲 0 fatal） |
| R4 | Definition Override | High | Resource Resolver 先行（P1-02） | ✅ P1-02 完成（上层迁移经 Overlay 全量验证） |
| R5 | Sign/Speed Semantics | Medium/High | Telemetry speed limit ground truth | 🔄 P1-09 裁剪：限速模型完成，telemetry 实测延后 P2 |
| R6 | TruckLib License | Medium | oracle-only 定位（ADR-005） | ✅ 已定案 |
| R7 | Dataset Schema Premature Freeze | Medium | 语义图稳定后冻结 v1 | ✅ P1-11 冻结 v1（Rust smoke 双向校验） |

P1 Gates（G1~G13）见 P1-map-compiler-plan.md §126–§138；Exit Criteria §139。

## §5 版本管理策略（GitHub）

- **仓库**：GitHub（账号 HaowenCang），本地仓库为唯一写入源。
- **分支模型**：`main`（可发布基线）+ 功能分支 `feat/<id>-<slug>`（如 `feat/A4-city-parser`）；P0 阶段可直接在 main 上小步提交，进入 P1 后强制分支 + PR。
- **提交规范**：`<ID>: <动词> <对象>`，如 `A4: 实现 sector 解析`；提交信息含变更要点与验证命令。
- **Tag**：P0 门通过 → `v0.1.0-p0`；此后按里程碑递增。
- **忽略**：`.pi-subagents/`、`.pi-glla/`、`target/`、`bin/ obj/`、`node_modules/`、提取的游戏资源（`vendor/` 若含大文件）。
- **文档随代码入库**：需求 v0.1/v0.2、本计划、可行性评估、格式笔记均在仓库内。

---

## §6 会话接续协议

1. 读本文件 §1（当前状态）→ §2（待确认事项）→ §3（任务状态表），确定续接点。
2. 更新任务状态为 `🔄 in_progress` 再动手。
3. 每个任务完成时更新状态并记录验证方式；遇到阻塞记 `🚫` + 原因。
4. 会话结束时：更新 §1 最后更新与摘要、提交 git。
5. 关键决策（语言、格式、实验结论）写入 `docs/decisions/`。

---

## §7 关键风险提示（v0.2 §67）

- 最高工程风险：Automatic Routing Graph（P0-A）——公开项目均需人工修复，本项目目标是无人工修复。
- 最高功能风险：Traffic Light ±1 s（P0-B）——SDK 无运行时相位通道；相位锚定 H1/H2 未定；**禁止在设计阶段预设时钟模型**（v0.2 §28）。
- 中等：speed camera、sign/exit number、speed-limit propagation、动态事件。
- 低：Telemetry、POI、A*、rerouting、TTS、LAN、UI。
- 合规（2026-08-09 修订）：项目已改 GPL-3.0，TruckLib（GPL-2.0）/TruckSim Maps（GPL-3.0）/ETS2LA（GPL-3.0）可直接复用；注意 GPL-2.0-only 与 GPL-3.0 的兼容性细节（TruckLib 需确认 or-later 条款或独立分发）。
