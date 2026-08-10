# ETS2Nav P2 — Navigation Core 开发规划

**文档状态**：正式执行基线  
**阶段**：P2 — Navigation Core  
**前置版本**：`v0.2.0-p1`  
**建议 P2 关门 Tag**：`v0.3.0-p2`  
**适用仓库**：`HaowenCang/ETS2Nav`  
**技术基线**：《Euro Truck Simulator 2 外部智能导航系统 v0.2》  
**P1 基线**：`P1-map-compiler-plan.md`、`docs/validation/p1-closeout-2026-08.md`

---

# 1. P2 阶段定义

P1 已经解决：

> ETS2 官方地图如何转换为独立、确定、可验证的 Navigation Dataset。

P2 解决：

> 如何利用 Navigation Dataset、实时 Telemetry 和当前导航目标，完成车辆实时定位、路径规划、多路线生成、路线进度跟踪、偏航重规划和转向引导。

完整链路定义为：

```text
Navigation Dataset
       +
SCS Telemetry
       +
Destination
       │
       ▼
┌───────────────────────────────┐
│       Navigation Core         │
│                               │
│ Dataset Runtime               │
│ Spatial Index                 │
│ Map Matching                  │
│ Routing Engine                │
│ Route Profiles                │
│ Alternative Routes            │
│ Route Progress                │
│ Rerouting                     │
│ Maneuver Generator            │
│ Signal Link                   │
└───────────────┬───────────────┘
                │
                ▼
        Navigation State
```

P2 完成后，应当存在真正能够在游戏运行期间工作的无 UI Headless Navigation Core。

---

# 2. P2 的核心出口

输入：

```text
navigation-data/
+
Telemetry shared memory
+
Destination
```

输出持续更新的：

```text
NavigationState
```

至少包括：

```text
vehicle_match
selected_route
alternative_routes
route_progress
remaining_distance
estimated_travel_time
current_road
next_maneuver
next_maneuver_distance
off_route_state
upcoming_signal
destination_state
```

P4 UI 不应自行计算这些结果。

UI 只负责：

> 显示 Navigation Core 已经确定的导航状态。

因此 P2 的核心边界为：

\[
\boxed{
\text{All navigation decisions end at P2}
}
\]

P3 再加入驾驶辅助策略。

P4 再加入最终用户界面。

---

# 3. 当前 P1 输入状态

P1 已正式关门。

全欧洲正式 Dataset 当前规模约：

```text
sector                 1,358
items                   2,530,050
raw map nodes           3,442,375
roads                   149,814
junctions               42,843
movements               171,453
routing edges           435,392
POI                     4,997
prefab types            2,432
```

正式产物：

```text
routing.graph
junction.graph
map.db
search.db
map.pmtiles
manifest.json
diagnostics.json
```

P2 不再读取 ETS2 `.scs` 文件作为常规运行路径。

---

# 4. P1 已经解决的关键问题

P2 不重复解决：

- HashFS；
- sector binary；
- SII parser；
- road look；
- prefab PPD parser；
- road direction；
- semantic movement；
- semaphore ID binding；
- POI extraction；
-官方 DLC overlay；
-地图 deterministic build。

P1 收官后：

```text
road direction
```

已经不再使用“全部道路双向”的近似。

```text
junction movement
```

也已经不再使用“prefab connector 全连接”的近似。

因此 P2 应当完全消费 P1 Semantic Graph，而不是重新解释原始地图。

---

# 5. P1 已知限制中与 P2 直接相关的项目

P2 开发前必须处理以下 P1 遗留项：

### L1 — LeftHandTraffic

UK / Ireland 尚未独立验证。

### L2 — Speed Telemetry

`speed-validator` 已完成，但 map speed 与游戏 Telemetry 的实车一致率尚未闭环。

### L3 — Roundabout

典型环岛尚未建立专项 semantic corpus。

### L4 — Maneuver TurnType

当前转向类型仍包含几何近似。

### L5 — Geometry

Dataset v1 没有为每条 Routing Edge 保存完整 polyline。

### L6 — Junction Geometry

PPD `CurvePath` 存在于 Semantic Model，但 Dataset v1 没有完整序列化。

### L7 — Ferry / Train

`RoutingEdgeKind` 已预留 Ferry/Train，但当前正式 `RoutingGraphBuilder` 实际只加入 Road 和 JunctionMovement。

### L8 — Speed Metadata Hot Path

`routing.graph` v1 没有直接保存 speed limit，P2 若直接运行需要查询 `map.db`。

### L9 — Runtime Signal Mapping

静态：

```text
JunctionMovement → PPD SemaphoreId
```

已经完成。

但：

```text
PPD SemaphoreId
↔
semaphore-bridge runtime light
```

尚未完成。

这些项目构成 P2 前置工作，而不是 P3 问题。

---

# 6. P2 正式范围

P2 必须完成：

- Dataset v2 契约；
-精确 edge geometry；
- ferry/train route connection；
- Rust Navigation Core；
- Dataset loader；
- Runtime compact graph；
- Spatial index；
- Telemetry adapter；
- telemetry trace recorder/replayer；
- Map Matching；
-路由起终点 snapping；
- Dijkstra oracle；
- A*；
-时间优先；
-距离优先；
-综合推荐；
- 2～3 条候选路线；
-路线去重；
-路线指标；
- route geometry；
- route progress；
-剩余距离；
- ETA 基础模型；
-偏航判断；
-自动重规划；
- maneuver generation；
-环岛出口识别；
- ferry/train maneuver；
-当前任务目的地解析；
- POI destination；
- runtime signal association；
- Headless Navigation State；
-全欧洲 routing regression；
-实时游戏集成测试；
-性能验收。

---

# 7. P2 明确非目标

P2 不负责：

-正式高德 UI；
-PC MapLibre 正式界面；
- Android/iOS；
-中文 TTS 最终实现；
-超速播报；
-测速摄像头播报；
-红灯刹车警告；
-即将绿灯警告；
-GLOSA 最终驾驶建议；
-动态限速视觉 OCR；
-出口路牌 OCR；
-车道级导航 UI；
-实时随机事件；
-TruckersMP；
-ProMods 适配；
-ALT/CH/CCH 最终性能优化。

其中部分数据可以在 P2 中准备，但正式驾驶辅助属于 P3。

---

# 8. P2 总体架构

推荐：

```text
                 navigation-data/
                      │
                      ▼
             ┌─────────────────┐
             │ Dataset Runtime │
             └────────┬────────┘
                      │
        ┌─────────────┼──────────────┐
        │             │              │
        ▼             ▼              ▼
 Routing Graph   Spatial Index   Metadata Store
        │             │              │
        └──────┬──────┘              │
               ▼                     │
          Map Matcher ◀──── Telemetry Adapter
               │
               ▼
         Vehicle Location
               │
       ┌───────┴────────┐
       ▼                ▼
 Routing Engine     Route Tracker
       │                │
       ▼                ▼
 Alternatives        Progress
       │                │
       └────────┬───────┘
                ▼
       Maneuver Generator
                │
                ▼
          Signal Linker
                │
                ▼
        Navigation State
```

---

# 9. P2 编程语言

Navigation Core 正式采用：

> Rust。

P1 Map Compiler 保持：

> C#。

两者通过：

> Navigation Dataset

通信。

禁止让 P2 直接引用 C# Map Compiler DLL。

---

# 10. 推荐 Rust Workspace

建议新增：

```text
nav-core/
├─ Cargo.toml
│
├─ crates/
│   ├─ nav-dataset/
│   ├─ nav-graph/
│   ├─ nav-spatial/
│   ├─ nav-telemetry/
│   ├─ nav-matcher/
│   ├─ nav-routing/
│   ├─ nav-guidance/
│   ├─ nav-signal/
│   └─ nav-core/
│
├─ tools/
│   └─ nav-core-cli/
│
└─ tests/
    ├─ fixtures/
    ├─ traces/
    └─ routes/
```

不建议进一步把每个小算法拆成独立 crate。

边界以：

-数据；
-空间；
-定位；
-路由；
-引导；

为主要划分。

---

# 11. P2-00 — Dataset Consumer Audit

P2 第一项工作不是写 A*。

先对 Dataset v1 执行消费侧审计。

目标：

> 判断 Navigation Core 所需字段是否全部存在。

审计至少覆盖：

```text
Map Matching
Route Cost
Route Geometry
Maneuver
Roundabout
Transit
Signal Link
```

---

# 12. Dataset v1 的主要不足

当前 `routing.graph` 可以完成基本 graph search：

```text
from
to
kind
length
source_uid
semaphore_id
flags
movement_id
```

但对于正式 P2 不够。

最明显缺失：

```text
edge polyline
speed limit hot metadata
road classification hot metadata
```

因此当前 Dataset v1：

> 可以用于 Dijkstra/A* 原型；

但不能直接作为正式 Map Matching Dataset。

---

# 13. 为什么必须增加 Edge Geometry

车辆 telemetry 给出的是真实世界坐标：

\[
(x,y,z).
\]

Map Matching 需要计算：

\[
d(
\text{vehicle},
\text{road polyline}
).
\]

如果只用道路两端：

```text
node0 ───────── node1
```

近似真实弯道：

```text
node0
   ╲
    ╲__
       ╲____ node1
```

在长弯、高速匝道和环岛附近会产生系统误差。

因此 P2 正式 Map Matching 的前提是：

\[
\boxed{\text{Routing Edge Geometry}}
\]

而不是：

\[
\boxed{\text{Edge Endpoint Geometry}}
\]

---

# 14. Dataset v2

建议在 P2 初期正式定义：

```text
ETS2NAV_DATASET_VERSION = 2
```

P1 tag 不修改。

Map Compiler `main` 向前演化产生 Dataset v2。

---

# 15. routing.graph v2

建议增加：

```text
geometry_offset
geometry_count

speed_limit
road_class / speed_class
```

Edge 建议概念模型：

```text
RoutingEdgeV2
{
    from
    to

    kind

    length

    source_uid

    geometry_offset
    geometry_count

    speed_limit_kph

    road_class

    semaphore_id
    movement_id

    flags
}
```

---

# 16. Geometry Table

采用统一：

```text
Point3[]
```

例如：

```text
x i32
y i32
z i32
```

继续使用：

\[
1/256
\]

固定点坐标。

Edge：

```text
geometry_offset
geometry_count
```

引用。

这样 Road 与 JunctionMovement 使用完全相同的 runtime geometry API。

---

# 17. Road Geometry

Map Compiler 应由 sector road 的真实曲线信息生成 polyline。

推荐：

> 编译阶段进行自适应曲线采样。

误差容限采用几何弦差控制，而不是固定每 1 m 一个点。

目标是：

-城市弯道足够精确；
-高速长直路不产生大量冗余点。

---

# 18. Junction Geometry

P1 已恢复：

```text
JunctionMovement.CurvePath
```

P2 Dataset v2 应将 movement curve chain 转换成世界坐标 polyline。

因此：

```text
entry
→
curve1
→
curve2
→
...
→
exit
```

在 Dataset 中成为完整 geometry。

---

# 19. Runtime Node Compaction

P1 全欧洲 Dataset 保存约 343 万 raw nodes，但真正路由边只有约 43.5 万。

P2 不应创建：

```text
Vec<Vec<Edge>>
```

覆盖数百万节点。

推荐在 Dataset v2 写入阶段或 Runtime loader 阶段进行：

> active routing node compaction。

只保留被 Routing Edge 引用的节点作为 runtime graph node。

仍保存：

```text
source UID
```

用于回溯。

---

# 20. CSR Graph

正式 Runtime Graph 推荐采用：

> CSR / adjacency array。

例如：

```text
node_offsets[N + 1]
edge_ids[E]
edges[E]
```

而不是：

```text
Vec<Vec<Edge>>
```

优点：

-更低内存；
-连续访问；
-缓存友好；
- A* 遍历快；
- deterministic。

反向邻接如果 Map Matching / rerouting 需要：

```text
reverse_offsets
reverse_edges
```

单独建立。

---

# 21. Speed Metadata

P1 当前 `map.db` 已含：

```text
road uid
speed_class
speed_limit
direction
length
```

P2 可以通过 SQLite join 恢复。

但 Route Search 是高频图遍历，不应在搜索过程中查询 SQLite。

因此：

> speed limit 必须在 Dataset load 阶段进入内存 Edge Metadata。

Dataset v2 更推荐直接把 speed limit 写入 hot graph。

---

# 22. 限速三态

沿用 P1 规则：

```text
-1 = unknown
 0 = unlimited
>0 = km/h
```

不得把：

```text
unknown
```

和：

```text
unlimited
```

混为一类。

---

# 23. Ferry / Train Graph

P1 已抽取 Ferry POI，但正式 Routing Graph 尚未加入 ferry/train edge。

P2-00 必须补齐：

```text
origin terminal
destination terminal
travel metadata
```

产生：

```text
RoutingEdgeKind::Ferry
RoutingEdgeKind::Train
```

否则跨海路线可能被错误判定：

> 无路线。

---

# 24. Transit Cost

Transit Edge 至少保存：

```text
travel_time
distance
transfer_penalty
```

如果游戏数据存在收费信息，可以同时保存：

```text
cost
```

但 V1 route profile 不要求优化金钱。

---

# 25. Dataset v2 Gate

Dataset v2 必须满足：

```text
Road geometry available
Junction geometry available
speed metadata available
Ferry/Train connectivity available
Rust reader PASS
deterministic build PASS
Europe build PASS
```

完成后才正式进入 Map Matching。

---

# 26. P2-01 — Rust Dataset Runtime

将现有：

```text
tools/dataset-reader-smoke
```

升级思路迁入正式：

```text
nav-dataset
```

但 smoke tool 保留。

---

# 27. Dataset Loader 验证

读取时必须验证：

```text
magic
dataset version
endianness
file size
offset
count
edge node range
geometry range
movement range
```

不得因为 Dataset 是本机生成就省略 bounds checking。

---

# 28. Dataset Version Policy

Runtime 应：

```text
support version == expected
```

否则：

```text
DatasetVersionMismatch
```

明确拒绝启动。

不能尝试“尽量读取”未知 binary version。

---

# 29. Manifest 验证

启动时读取：

```text
manifest.json
```

检查：

```text
dataset version
game version
scope
required files
```

如果 Dataset fingerprint 与当前游戏不一致：

> 可以启动调试模式；

但正式导航模式应提示需要重新编译地图。

---

# 30. Metadata Runtime

建议：

```text
MetadataStore
```

一次性读取：

```text
map.db
search.db
```

Route Search hot loop 不直接访问 SQLite。

SQLite 只用于：

- POI；
-搜索；
-道路 metadata；
-低频查询。

---

# 31. P2-02 — Telemetry Runtime

复用 P0：

```text
scs-nav-bridge
```

共享内存协议。

Rust 增加：

```text
nav-telemetry
```

读取：

```text
position
heading
speed
simulation time
pause
speed limit
job destination
fuel/rest future fields
```

---

# 32. Telemetry Snapshot

统一类型：

```text
TelemetrySnapshot
{
    sequence

    sim_time

    position
    heading

    speed

    telemetry_speed_limit

    paused

    job
}
```

其他模块不得直接读取 Shared Memory。

---

# 33. Telemetry Staleness

如果 sequence 长时间不更新：

```text
TelemetryState::Stale
```

Navigation Core：

-停止推进 route progress；
-不触发 rerouting；
-保留现有 route。

---

# 34. Pause

游戏暂停时：

```text
NavigationState.paused = true
```

不重新匹配、不更新 ETA 驾驶进度。

---

# 35. Teleport / Load 检测

至少检测：

```text
simulation time reset
position discontinuity
sequence restart
```

发生后：

```text
clear map matcher history
clear route progress anchor
clear signal association
```

但：

> 保留当前 destination/profile。

重新获得位置后自动重规划。

---

# 36. Telemetry Trace Recorder

P2 必须建立：

```text
nav-trace-recorder
```

记录：

```text
timestamp
position
heading
speed
speed_limit
pause
job
signal snapshot optional
```

输出：

```text
*.navtrace
```

或 JSONL/binary。

---

# 37. Trace Replay

建立：

```text
nav-core-cli replay
```

允许：

> 不运行 ETS2 就重复运行 Map Matching / Rerouting。

这对 P2 调试非常重要。

---

# 38. P2 前置验证 — Left Hand Traffic

在进入欧洲大规模 route regression 前必须单独测试：

-英国；
-爱尔兰。

目标：

> 确认当前 P1 RoadDirection 逻辑没有系统性反向。

至少建立：

```text
UK route corpus
Ireland route corpus
```

---

# 39. LeftHandTraffic Gate

验证：

- motorway；
-普通城市道路；
-环岛；
-高速出入口。

硬条件：

```text
0 known reverse route
```

如果失败：

> 修复 Map Compiler SemanticMapBuilder 并重建 Dataset v2。

---

# 40. P2 前置验证 — Speed

使用 P1 已完成的：

```text
speed-validator
```

采集：

-城市；
-高速；
-普通道路；
-不同国家。

对照：

```text
map speed
vs
telemetry speed
```

---

# 41. Speed Gate

不在规划阶段预设一个未经实测支持的百分比。

第一轮实测后统计：

```text
exact match
known systematic mismatch
unknown limit
unlimited limit
```

所有系统性误差必须分类。

只有：

> 不存在未解释的大规模系统偏差

后，Fastest Profile 才进入正式 Gate。

---

# 42. P2-03 — Spatial Index

Map Matching 需要快速找到车辆附近道路。

新增：

```text
nav-spatial
```

建议建立：

> Edge bounding-box R-tree。

索引对象仅包括：

```text
Road
JunctionMovement
```

不包括 ferry/train。

---

# 43. Spatial Entry

```text
SpatialEdge
{
    edge_id
    bbox
}
```

具体 polyline 存在 Dataset Runtime。

查询：

```text
query_radius(position, radius)
```

返回候选 edge。

---

# 44. Adaptive Search Radius

初始化定位：

> 使用较大 radius。

连续导航：

> 使用较小 radius。

发生：

- teleport；
- matcher lost；

重新扩大搜索范围。

不应每帧扫描所有 edge。

---

# 45. P2-04 — Map Matching

Map Matching 输入：

```text
TelemetrySnapshot
Previous Match State
Spatial Index
```

输出：

```text
MapMatch
{
    edge_id
    offset
    projected_position
    direction
    confidence
}
```

---

# 46. Map Matching 不能采用最近道路法

在：

-高速上下层；
-平行辅路；
-对向道路；
-复杂互通；

单纯：

\[
\operatorname*{argmin}_e d(x,e)
\]

会出现错误。

因此至少需要：

```text
distance
heading
previous edge
topology continuity
speed
```

共同评分。

---

# 47. Candidate Score

概念上：

\[
S(e)=
w_d S_d+
w_h S_h+
w_t S_t+
w_r S_r.
\]

其中：

- \(S_d\)：车辆与 polyline 距离；
- \(S_h\)：车辆航向与 edge tangent；
- \(S_t\)：与上一 edge 的拓扑连续性；
- \(S_r\)：当前 route corridor bias。

具体权重由 trace calibration 决定。

---

# 48. Heading

不是用：

```text
from node → to node
```

作为整条 road heading。

应使用：

> 投影点附近 polyline tangent。

否则长弯道中 heading 判断会错误。

---

# 49. Rolling Matcher

V1 推荐：

> rolling multi-candidate tracker。

没有必要第一版直接实现复杂全局 HMM。

每一帧保留 Top-K：

```text
candidate state
score
parent
```

滑动窗口后确定最优路径。

本质上是局部 Viterbi。

---

# 50. Route Bias

已经有 active route 时：

> route 上或 route corridor 内 edge 获得有限 bonus。

但 route bias 不能压过明显的物理证据。

否则车辆偏航后 matcher 会一直“吸附”在旧路线。

---

# 51. Map Match Confidence

输出：

```text
HIGH
MEDIUM
LOW
UNMATCHED
```

只有 HIGH/MEDIUM 允许：

- route progress；
- off-route 判断。

LOW 期间：

> 保持最近稳定状态。

---

# 52. Map Matching Corpus

必须采集：

-普通直路；
-城市路口；
-高速；
-平行道路；
-上下层互通；
-服务区；
-公司园区；
-环岛；
-英国左侧道路。

---

# 53. Map Matching Gate

硬条件：

-人工标注关键 checkpoint：0 wrong carriageway；
-关键互通：0 persistent parallel-road swap；
-停车/暂停不漂移；
- teleport 后能够重新锁定；
-跨 junction 连续；
-进入服务区可正确切换道路。

整体 edge accuracy 的数值目标在首轮标注 corpus 后正式冻结。

---

# 54. P2-05 — Snap Model

Routing 起点和任意地图点击目的地通常不恰好位于 node。

因此增加：

```text
SnapPoint
{
    edge_id
    offset
    position
}
```

---

# 55. 起点 Snap

当前车辆已经 Map Matched：

> 直接使用当前 edge + offset。

不要再次 nearest-node。

---

# 56. 目的地 Snap

POI：

> 优先使用 P1 `access_node`。

任意地图点击：

> 使用 Spatial Index 匹配最近可路由 edge。

---

# 57. Virtual Start / Goal

不要修改全局 graph。

Route Search 支持虚拟起点：

```text
VirtualStart
   ├→ edge remaining forward
   └→ edge remaining backward（若允许）
```

目的地同理。

这样能正确处理：

> 车辆已经位于道路中部。

---

# 58. P2-06 — Routing Cost Model

统一定义：

```text
EdgeCostProvider
```

概念接口：

```text
cost(edge, context) -> nonnegative cost
```

Route Search 与路线策略解耦。

---

# 59. Distance Profile

定义：

\[
C_e=L_e.
\]

仅优化：

> 总行驶距离。

但仍遵守所有 graph legality。

---

# 60. Time Profile

Road：

\[
T_e=\frac{L_e}{v_e}.
\]

其中 \(v_e\) 根据：

- speed limit；
- road class；
- fallback；

确定。

Movement：

\[
T_m=
\frac{L_m}{v_\mathrm{junction}}
+
P_\mathrm{signal}
+
P_\mathrm{turn}.
\]

---

# 61. Unknown Speed

如果：

```text
speed_limit = -1
```

不得：

> 当成无限速。

使用：

```text
fallback speed model
```

并记录：

```text
route contains estimated-speed edge
```

供 diagnostics。

---

# 62. Unlimited Speed

如果：

```text
speed_limit = 0
```

不能数学上设：

\[
v=\infty.
\]

使用：

> profile-defined practical free-flow cap。

这是 ETA 模型参数，而不是法律限速。

---

# 63. Traffic Light Static Cost

P2 路由时对远方信号灯不需要知道当前实时 phase。

只要：

```text
movement.SemaphoreId >= 0
```

即可增加统计 delay。

初版采用：

```text
configurable expected signal delay
```

而不是伪造精确未来等待时间。

实际动态灯状态只用于临近路口导航和 P3 驾驶辅助。

---

# 64. Turn Penalty

对：

-急转；
-U-turn；
-复杂 movement；

允许增加少量时间成本。

参数必须：

> 可配置并通过路线结果观察调节。

---

# 65. GpsAvoid

P1 已保留：

```text
GpsAvoid
```

默认：

> 高 penalty，但不是绝对不可通行。

这样在唯一通路时仍然可以经过。

---

# 66. Secret

默认 Route Profile 建议：

> 强 penalty 或排除。

具体行为在实际地图 corpus 中验证后冻结。

不得在没有实证前简单把 `Secret` 解释成“禁止车辆”。

---

# 67. NoAiVehicles

仅作为 metadata。

不得直接推导：

> 玩家卡车禁止通行。

除非进一步地图语义证明。

---

# 68. Balanced Profile

定义：

\[
C_e=
w_tT_e+
w_dD_e+
P_\mathrm{signal}+
P_\mathrm{road}+
P_\mathrm{avoid}.
\]

目标是：

> 类似现实地图软件的“推荐路线”。

具体权重通过路线 corpus 调参。

---

# 69. 默认 Route Profiles

P2 正式支持：

```text
FASTEST
SHORTEST
BALANCED
```

对应当前产品需求。

后续可以增加：

```text
FEWER_SIGNALS
MOTORWAY_PREFERRED
AVOID_TOLL
```

但不阻断 P2 Gate。

---

# 70. P2-07 — Dijkstra Oracle

正式 A* 之前先实现：

> Dijkstra reference search。

作用不是生产性能。

作用是：

> 验证 A* 最优性。

---

# 71. Dijkstra Regression

随机 OD：

\[
C_{\mathrm{A*}}
=
C_{\mathrm{Dijkstra}}
\]

在浮点容差范围内。

这应成为 Routing Engine 最重要的算法回归之一。

---

# 72. P2-08 — A*

正式 route search 使用：

> A*。

Shortest heuristic：

\[
h(n)=d(n,g).
\]

Fastest heuristic：

\[
h(n)=\frac{d(n,g)}{v_{\max}}.
\]

Balanced：

使用仅包含已知非负下界项的 heuristic。

---

# 73. Heuristic 原则

必须保证：

\[
h(n)\leq h^\*(n).
\]

不能为了更快而引入可能高估的 heuristic，导致：

> Fastest 路线不再保证最优。

---

# 74. A* Runtime Data

推荐预分配：

```text
g_score[]
parent_edge[]
visit_generation[]
```

使用：

```text
generation counter
```

避免每次搜索都清空整个数组。

---

# 75. Priority Queue

使用 binary heap。

不要在 P2 第一版直接引入：

-CH；
-CCH；
-多级图；
-Landmark preprocessing。

当前约 43.5 万边规模应先实测 A*。

---

# 76. Routing Result

```text
Route
{
    profile

    edges[]
    geometry[]

    legs[]

    metrics

    maneuvers[]
}
```

---

# 77. Route Metrics

至少：

```text
distance_m
estimated_time_s

road_edge_count
junction_count
signal_count

ferry_count
train_count

gps_avoid_distance
unknown_speed_distance
```

这些指标同时供：

> 多路线比较页面。

---

# 78. ETA

P2 ETA 定义为：

> 静态自由流 ETA。

不声称是真实交通 ETA。

它考虑：

-道路限速；
-静态路口成本；
-红绿灯统计 delay；
-ferry/train。

不考虑：

-实时拥堵；
-随机事故；
-玩家驾驶风格。

---

# 79. P2-09 — Alternative Routes

产品要求：

> 2～3 条不同策略路线。

初版优先运行：

```text
FASTEST
SHORTEST
BALANCED
```

得到最多三条。

---

# 80. Route Deduplication

如果：

```text
FASTEST ≈ BALANCED
```

不能向用户显示两条几乎完全一样的路线。

定义 overlap：

\[
O(A,B)=
\frac{\text{shared route length}}
{\min(L_A,L_B)}.
\]

高 overlap 路线视为重复。

阈值通过 corpus 调整。

---

# 81. Alternative Generation

若三个 profile 最终只有 1～2 条独立路线：

可以对最优路线上的 edge 添加：

```text
overlap penalty
```

重新运行一次 A*。

目标：

> 尝试找到真正不同但合理的路线。

不是强制凑三条。

---

# 82. Alternative Quality

候选路线还必须满足：

-不存在明显回头路；
-成本没有异常大幅劣化；
-路线结构合法。

如果不存在合理备选：

> 只返回一条。

---

# 83. P2-10 — Route Progress

路线选定后构建：

```text
RouteTracker
```

保存：

```text
current route edge index
edge offset
distance travelled
remaining distance
next maneuver index
```

---

# 84. Route Matching

MapMatch 输出 edge 后：

> 在当前 route 的局部窗口内寻找对应 route edge。

不能每帧扫描整条跨欧洲 route。

---

# 85. Remaining Distance

预计算 route suffix：

```text
suffix_distance[i]
```

于是：

\[
D_\mathrm{remaining}
=
D_\mathrm{edge,remaining}
+
D_\mathrm{suffix}.
\]

每帧 O(1)。

---

# 86. ETA Update

类似地：

```text
suffix_time[i]
```

得到：

\[
T_\mathrm{remaining}.
\]

可根据当前速度做短时平滑，但 P2 不实现复杂驾驶风格学习。

---

# 87. Route Progress Monotonicity

正常驾驶时 route progress 应大致单调增加。

允许：

-倒车；
-停车；
-U-turn；

但不得因 matcher 在平行 edge 间跳动导致 progress 频繁前后跃迁。

---

# 88. P2-11 — Off-route Detection

不能：

```text
matched_edge != route_edge
→ reroute
```

必须设置 hysteresis。

---

# 89. Off-route Evidence

至少综合：

```text
连续帧不在 route corridor
沿非 route edge 前进距离
heading
route recovery possibility
```

---

# 90. Off-route State

定义：

```text
ON_ROUTE
SUSPECTED_OFF_ROUTE
OFF_ROUTE
REROUTING
```

避免状态抖动。

---

# 91. Rerouting

确认：

```text
OFF_ROUTE
```

后：

```text
current SnapPoint
+
same destination
+
same profile
→ new Route
```

---

# 92. Rerouting 时不阻塞 Telemetry

Route Search 放在 worker thread。

实时 telemetry / map matching 不应因 A* 暂停。

---

# 93. Rerouting Gate

目标：

> 偏航确认后约 1 s 内产生新路线。

这是原 v0.2 的正式性能目标。

---

# 94. P2-12 — Maneuver Generator

P2 必须把 Route Edge Sequence 转换为：

```text
Maneuver[]
```

这是 P4 导航卡片和中文 TTS 的直接输入。

---

# 95. Maneuver 类型

V1 至少：

```text
Depart
Continue

SlightLeft
Left
SharpLeft

SlightRight
Right
SharpRight

KeepLeft
KeepRight

UTurn

EnterMotorway
ExitMotorway

Roundabout
Ferry
Train

Arrive
```

---

# 96. Movement 优先

在 prefab 内存在：

```text
JunctionMovement.TurnType
```

时优先使用 semantic movement。

不应仅使用两个 road endpoint 的夹角。

---

# 97. Geometric Refinement

P1 `TurnType` 仍存在几何近似。

P2 利用 Dataset v2 的真实 movement polyline 计算：

```text
entry tangent
exit tangent
curve geometry
```

进一步细分：

```text
slight
normal
sharp
```

---

# 98. Maneuver Suppression

不是每一个 JunctionMovement 都应该播报。

例如：

> 同一道路自然轻微弯曲

不应产生：

> 右转。

需要：

```text
road continuity
junction topology
bearing change
```

共同判断。

---

# 99. Keep Left / Keep Right

高速分叉中：

> 主路延续 vs 分叉

比单纯角度重要。

算法需要比较：

- route outgoing edge；
-其他 outgoing edges；
- road class continuity。

---

# 100. P2-13 — Roundabout

P2 必须建立专项 Roundabout Corpus。

这是 P1 未完成的语义验证。

---

# 101. Roundabout Detection

不能只根据 prefab 名字是否包含：

```text
roundabout
```

判断。

优先使用：

- junction movement topology；
-内部 geometry；
- connector distribution；
-路线通行结构。

Prefab token 可以作为辅助 evidence。

---

# 102. Roundabout Exit Count

从车辆 entry 开始：

> 按实际合法行驶方向遍历可能出口。

规划 route 使用的出口在序列中编号：

```text
1
2
3
...
```

输出：

```text
从环岛第 N 个出口驶出
```

---

# 103. Left-hand Roundabout

UK/Ireland 环岛方向必须单独验证。

这也是 LeftHandTraffic Gate 的一部分。

---

# 104. Roundabout Gate

建立至少多个不同国家、不同出口数、不同几何 prefab 的 corpus。

要求：

```text
entry 正确
direction 正确
exit count 正确
route geometry 连续
```

---

# 105. P2-14 — Transit Guidance

Ferry / Train edge 进入正式 route 后：

产生：

```text
BoardFerry
BoardTrain
ExitFerry
ExitTrain
```

或统一 leg model。

---

# 106. Route Leg

建议：

```text
RouteLeg::Drive
RouteLeg::Ferry
RouteLeg::Train
```

这样 UI 不需要从 edge sequence 猜测运输方式。

---

# 107. P2-15 — Destination Resolver

P2 需要统一：

```text
Destination
```

模型。

来源：

```text
POI
Current Job
Map Click
Coordinate
```

---

# 108. POI Destination

使用：

```text
search.db.poi.access_node
```

作为正式导航目标。

不要使用 POI visual position。

---

# 109. Current Job Destination

Telemetry 已经提供：

```text
destination city/company
```

P2 建立：

```text
JobDestinationResolver
```

映射到：

```text
company POI
→ access_node
```

---

# 110. Destination Resolution Failure

如果：

```text
company ID
```

无法映射：

返回：

```text
DestinationNotFound
```

不能随意选择同城另一家公司。

---

# 111. P2-16 — Signal Runtime Link

P1 已实现：

```text
JunctionMovement
→
PPD SemaphoreId
```

P0 已实现：

```text
runtime signal array
```

P2 负责桥接二者。

---

# 112. Signal Link 目标

对于当前 route 中下一个受控 movement：

```text
movement.SemaphoreId
```

找到：

> 当前游戏中实际对应灯组。

最终输出：

```text
UpcomingSignal
{
    junction
    semaphore_group

    runtime_state
    remaining_time
    confidence
}
```

---

# 113. Static Signal Geometry

Dataset v2 建议额外保存：

```text
signal head position
signal direction
PPD SemaphoreId
```

这样 runtime association 可以使用：

```text
position
direction
junction proximity
```

匹配。

---

# 114. Runtime Signal Association

候选评分：

\[
S=
w_pS_\mathrm{position}
+
w_hS_\mathrm{heading}
+
w_jS_\mathrm{junction}.
\]

只在当前车辆附近和当前 route junction 中匹配。

不需要全欧洲所有灯同时建立 runtime mapping。

---

# 115. Signal Link Confidence

定义：

```text
VERIFIED
PROBABLE
UNKNOWN
```

只有 VERIFIED：

> 可以交给 P3 做 ±1 s countdown 和 GLOSA。

P2 自身不进行驾驶提醒。

---

# 116. UseProfile 限制

P1 已确认许多 PPD semaphore 的显式 major/minor type 不可见。

P2 不需要为了 navigation linkage 强行推断 major/minor。

首先保证：

> 当前 movement 对应正确灯组。

---

# 117. P2 与 P3 的信号灯边界

P2：

```text
Which signal controls this movement?
What is its runtime state?
```

P3：

```text
Should we warn?
Should we display countdown?
What GLOSA speed?
```

严格分离。

---

# 118. P2-17 — Navigation State Machine

建立中央：

```text
NavigationSession
```

状态：

```text
Idle
DestinationSet
Planning
Navigating
SuspectedOffRoute
Rerouting
Arrived
Paused
LostPosition
Error
```

---

# 119. NavigationSession

负责协调：

```text
Telemetry
MapMatcher
Router
RouteTracker
ManeuverGenerator
SignalLinker
```

各模块本身尽量保持纯算法。

---

# 120. NavigationSnapshot

对外稳定输出：

```text
NavigationSnapshot
{
    session_state

    vehicle

    map_match

    route
    alternatives

    progress

    next_maneuver

    upcoming_signal

    destination

    diagnostics
}
```

这是未来：

- Desktop UI；
- WebSocket；
- Mobile UI；

的统一源。

---

# 121. Navigation Core 不直接画 UI

P2 不包含：

```text
MapLibre
React
Tauri
Android
```

Debug Viewer 可以存在。

生产 Navigation Core 必须 headless。

---

# 122. P2-18 — Debug CLI

新增：

```text
ets2nav-core
```

开发命令。

---

# 123. dataset

```text
ets2nav-core dataset info <dataset>
```

显示：

- nodes；
-edges；
-geometry；
-POI；
-version。

---

# 124. route

```text
ets2nav-core route
  --from <x,z>
  --to <x,z|poi>
  --profile fastest
```

输出：

```text
distance
ETA
signals
edges
maneuvers
```

以及可选：

```text
GeoJSON
```

---

# 125. replay

```text
ets2nav-core replay trace.navtrace
```

运行：

- Map Matching；
- Route Progress；
- Rerouting。

---

# 126. live

```text
ets2nav-core live
```

连接现有 Telemetry Bridge。

控制台持续输出：

```text
MATCH
ROAD
ROUTE_PROGRESS
MANEUVER
SIGNAL
```

---

# 127. bench

```text
ets2nav-core bench
```

执行：

- route benchmark；
-map match benchmark；
-dataset load benchmark。

---

# 128. P2 Regression Corpus

P2 建立四类 corpus。

### Routing Corpus

固定 OD + profile。

### Matching Corpus

Telemetry trace + 标注 checkpoints。

### Maneuver Corpus

junction/roundabout expected instruction。

### Runtime Corpus

pause/load/teleport/reroute/signal。

---

# 129. Routing Correctness Tests

每条 Route 必须保证：

```text
edge[i].to == edge[i+1].from
```

并：

```text
cost >= 0
distance >= 0
```

所有 movement：

> 已存在于 Dataset。

---

# 130. A* Oracle Tests

随机选取一定数量 OD：

```text
A*
vs
Dijkstra
```

要求：

\[
|C_A-C_D|<\epsilon.
\]

这是算法正确性的硬 Gate。

---

# 131. Europe Route Regression

至少：

```text
≥1000 deterministic OD
```

执行 A*。

检查：

-成功；
-连续；
-合法；
-无 NaN；
-无 infinite loop。

---

# 132. Dijkstra Sample

Dijkstra 较慢，因此不要求对全部 1000 OD 对照。

建议：

```text
100～200
```

代表性 OD 做最优成本 oracle。

---

# 133. Cross-country Corpus

固定路线至少覆盖：

- Germany；
-France；
-Spain；
-Italy；
-Scandinavia；
-Balkans；
-UK；
-Ireland；
-ferry route。

确保不同 DLC 都参与。

---

# 134. Alternative Route Corpus

选择：

> 确实存在多条合理路径的 OD。

验证：

- overlap；
-成本差；
-策略差异。

不能只验证“返回数组长度 = 3”。

---

# 135. Reroute Corpus

至少包括：

```text
提前拐错
错过高速出口
服务区驶入
公司园区驶入
掉头
teleport
load save
```

---

# 136. Arrival Detection

不能只判断：

```text
remaining_distance == 0
```

使用：

```text
destination proximity
+
route progress
+
vehicle state
```

避免车辆从目的地附近高速道路经过时误判到达。

---

# 137. Arrival State

输出：

```text
ARRIVING
ARRIVED
```

P4 可分别显示：

> 即将到达目的地

和：

> 已到达目的地。

---

# 138. P2 性能目标

保持 v0.2 原指标，同时增加内部算法指标。

---

# 139. Dataset Load

目标：

> 全欧洲 Dataset 在现代 SSD 上数秒级完成。

首轮实现后记录：

```text
cold load
warm load
peak RAM
```

再冻结正式阈值。

---

# 140. Map Matching

目标频率：

```text
10–20 Hz
```

单次 matcher 应远小于 telemetry 周期。

建议目标：

```text
p99 < 10 ms
```

如果明显低于该值则不继续过度优化。

---

# 141. Single Route

正式目标：

```text
典型 < 500 ms
```

---

# 142. Multi-route

FASTEST + SHORTEST + BALANCED：

```text
典型 < 1 s
```

极端情况：

```text
< 2 s
```

---

# 143. Rerouting

偏航确认之后：

```text
≈ 1 s
```

内得到新 route。

---

# 144. Runtime CPU

仅：

```text
Navigation Core
+
Telemetry
```

目标总 CPU：

> 尽量保持约 1～3% 范围。

具体以用户实际硬件基准为准。

---

# 145. Runtime RAM

总体目标：

```text
< 500 MB
```

但由于 P2 引入 geometry，必须专门记录：

```text
raw dataset memory
compact graph memory
spatial index
route state
```

---

# 146. Game FPS Regression

继续使用 P0 PresentMon 流程。

比较：

```text
ETS2 only
vs
ETS2 + full P2 core
```

要求：

-平均 FPS 回退不超过约 1～2%；
- 1% low 回退不超过 2%。

若超过则 profile。

---

# 147. Concurrency Model

推荐三个主要执行域。

```text
Realtime Loop
    telemetry
    matcher
    route progress

Route Worker
    A*
    alternatives
    rerouting

Signal Reader
    semaphore shared memory
```

---

# 148. Route Search 不进入实时线程

即使一次搜索只需要几十毫秒，也不得阻塞：

```text
Telemetry → Map Matching
```

实时 loop。

---

# 149. Snapshot 交换

Navigation State 推荐：

> immutable snapshot / channel。

避免 UI 或未来 server 获得内部可变状态引用。

---

# 150. Error Model

建立统一：

```text
NavigationError
```

至少：

```text
DatasetMissing
DatasetVersionMismatch
DatasetCorrupt

TelemetryUnavailable
TelemetryStale

PositionUnmatched

DestinationNotFound
DestinationUnreachable

RouteNotFound

SignalUnavailable
```

---

# 151. Graceful Degradation

例如：

### 无 signal

导航继续：

```text
upcoming_signal = None
```

### 无 speed

路线仍可算：

```text
estimated_speed = fallback
```

### Matcher LOW

保留最近稳定导航状态。

### Search route failed

不得崩溃。

---

# 152. Diagnostics

P2 Runtime 应可记录：

```text
match candidate scores
selected edge
route search stats
expanded nodes
route cost breakdown
offroute evidence
signal match candidates
```

默认 Release 不高频写磁盘。

Debug 模式开启。

---

# 153. Route Cost Explainability

对于 debug route：

```text
distance = ...
drive time = ...
signal penalty = ...
gps avoid penalty = ...
```

全部可输出。

这对后续调“推荐路线”非常重要。

---

# 154. P2 工作包总表

| ID | 工作包 | 主要目标 | 阻塞性 |
|---|---|---|---|
| P2-00 | Dataset consumer audit | 冻结 P2 数据需求 | 是 |
| P2-01 | Dataset v2 | geometry/speed/transit | 是 |
| P2-02 | Rust runtime | loader + compact CSR | 是 |
| P2-03 | Telemetry/trace | 实时输入与回放 | 是 |
| P2-04 | P1 liability gates | UK/speed/roundabout baseline | 是 |
| P2-05 | Spatial index | edge candidate query | 是 |
| P2-06 | Map Matching | 实时道路定位 | 是 |
| P2-07 | Snap model | 任意起终点 | 是 |
| P2-08 | Cost profiles | fastest/shortest/balanced | 是 |
| P2-09 | Dijkstra/A* | 正式 route search | 是 |
| P2-10 | Alternatives | 2～3 条策略路线 | 是 |
| P2-11 | Route tracker | progress/distance/ETA | 是 |
| P2-12 | Rerouting | 偏航与重规划 | 是 |
| P2-13 | Maneuver | turn guidance | 是 |
| P2-14 | Roundabout/transit | 特殊引导 | 是 |
| P2-15 | Destination resolver | POI/job/map click | 是 |
| P2-16 | Signal linker | static↔runtime signal | 部分 |
| P2-17 | Navigation session | headless state machine | 是 |
| P2-18 | Europe regression | 全图正确性 | 是 |
| P2-19 | Performance/closeout | P2 Gate | 是 |

---

# 155. 推荐实施顺序

```text
P2-00 Dataset Audit
        ↓
P2-01 Dataset v2
        ↓
P2-02 Rust Runtime
        ↓
P2-03 Telemetry / Trace
        ↓
P2-04 UK + Speed + Roundabout Baseline
        ↓
P2-05 Spatial Index
        ↓
P2-06 Map Matching
        ↓
P2-07 Snap
        ↓
P2-08 Cost Model
        ↓
P2-09 Dijkstra + A*
        ↓
P2-10 Alternatives
        ↓
P2-11 Route Tracker
        ↓
P2-12 Rerouting
        ↓
P2-13 Maneuver
        ↓
P2-14 Roundabout / Transit
        ↓
P2-15 Destination Resolver
        ↓
P2-16 Signal Link
        ↓
P2-17 Full Navigation Session
        ↓
P2-18 Europe Regression
        ↓
P2-19 Performance / Closeout
```

---

# 156. 为什么 Map Matching 必须早于正式 Routing Integration

算法上可以先写 A*。

但产品上：

> Navigation Core 的真实起点来自车辆当前位置。

如果没有 Map Matching，就无法正确解决：

-车辆位于哪条道路；
-所在方向；
-road offset；
-是否偏航。

因此建议：

> Dataset v2 后优先完成 Map Matching。

A* 可以作为独立分支并行实现，但不要把整个 P2 Gate 建立在“两个 node 之间能算路线”这一层。

---

# 157. P2 PR / Branch 策略

P1 收官报告已经记录：

> 原计划强制 PR，但实际多个工作包直接提交 main。

P2 应恢复计划中的分支策略：

```text
main
+
feat/p2-xx-...
```

除文档热修外：

> 不直接在 main 开发大型功能。

---

# 158. 推荐 Branch 顺序

```text
feat/p2-00-dataset-audit
feat/p2-01-dataset-v2
feat/p2-02-rust-runtime
feat/p2-03-telemetry-trace
feat/p2-04-preflight-gates
feat/p2-05-spatial-index
feat/p2-06-map-matching
feat/p2-07-snap-model
feat/p2-08-route-cost
feat/p2-09-astar
feat/p2-10-alternatives
feat/p2-11-route-tracker
feat/p2-12-rerouting
feat/p2-13-maneuvers
feat/p2-14-roundabout-transit
feat/p2-15-destination
feat/p2-16-signal-link
feat/p2-17-navigation-session
feat/p2-18-regression
feat/p2-19-closeout
```

---

# 159. 每个 PR 的最低要求

Rust：

```text
cargo fmt --check
cargo clippy
cargo test
```

Map Compiler 改动：

```text
dotnet build
dotnet test
run-p1-tests.bat
```

Dataset schema 改动必须：

```text
C# writer
+
Rust reader
```

同时修改。

---

# 160. Dataset v2 兼容规则

修改格式字段时：

> 先让旧 Rust reader test 失败。

然后更新读取端。

这是 P1 关门时已经证明有效的双实现约束。

---

# 161. P2 CI

GitHub Actions 建议：

```text
Rust build
Rust unit tests
Clippy
Dataset fixture tests
Routing oracle tests
Map matching replay tests
```

完整 ETS2 Europe Dataset 因版权和体积原因：

> 不进 CI。

使用小型合法 fixtures。

---

# 162. 本地 Full Regression

建立：

```text
run-p2-tests.bat
```

内部调用：

```text
run-p1-tests.bat
cargo test
dataset-v2 smoke
route regression
map-match replay
maneuver corpus
signal-link test
performance smoke
```

---

# 163. P2 Gate — G0 P1 Compatibility

要求：

```text
P1 regression 仍然 ALL PASS
```

P2 对 Map Compiler 的修改不得破坏 P1。

---

# 164. P2 Gate — G1 Dataset v2

必须：

```text
Europe build PASS
geometry complete
speed metadata complete
ferry/train route edges available
Rust reader PASS
determinism PASS
```

---

# 165. P2 Gate — G2 Left-Hand Traffic

UK/Ireland：

```text
0 known reverse route
```

---

# 166. P2 Gate — G3 Speed

必须完成真实游戏：

```text
Telemetry speed
vs
Map speed
```

验证。

所有大规模系统性 mismatch 都有解释或修正。

---

# 167. P2 Gate — G4 Map Matching

关键 corpus：

```text
0 known wrong carriageway
0 persistent parallel-road lock error
junction continuity PASS
teleport recovery PASS
```

---

# 168. P2 Gate — G5 Routing Correctness

A*：

```text
100～200 deterministic OD
```

与 Dijkstra：

> 最优成本一致。

全欧洲：

```text
≥1000 OD
```

route structural validation PASS。

---

# 169. P2 Gate — G6 Route Profiles

必须能够产生：

```text
Fastest
Shortest
Balanced
```

且指标解释正确。

不要求每一个 OD 三条都不同。

---

# 170. P2 Gate — G7 Alternatives

存在合理多路径的 corpus 中：

> 能找到 2～3 条具有实际差异的路线。

高重合候选能够去重。

---

# 171. P2 Gate — G8 Route Tracking

实时 trace：

```text
remaining distance
next maneuver distance
route edge index
```

稳定。

无明显跳跃。

---

# 172. P2 Gate — G9 Rerouting

典型偏航：

> 自动确认并产生新路线。

目标：

```text
≈1 s
```

---

# 173. P2 Gate — G10 Maneuver

典型：

-十字口；
-T 字口；
-高速入口；
-高速出口；
-分叉；
-U-turn；
-环岛；
-ferry/train；

全部产生正确语义。

---

# 174. P2 Gate — G11 Roundabout

典型 corpus：

```text
entry
direction
exit number
```

全部正确。

包括：

> UK 左侧环岛。

---

# 175. P2 Gate — G12 Destination

至少：

```text
POI
company
current job
map coordinate
```

均能建立导航。

---

# 176. P2 Gate — G13 Signal Link

对于已知 P0 信号灯测试路口：

```text
route movement
→
PPD group
→
runtime signal
```

能够稳定关联。

错误关联：

> 不得以 VERIFIED 输出。

---

# 177. P2 Gate — G14 Performance

目标：

```text
single route typical <500 ms
3-profile routing typical <1 s
reroute ~1 s
map matching 10–20 Hz
core CPU ~1–3%
RAM <500 MB
```

并重新执行 PresentMon FPS 基准。

---

# 178. P2 Gate — G15 Full Session

最终必须实际在 ETS2 中完成一次完整流程：

```text
启动游戏
↓
Core 自动读取 telemetry
↓
定位
↓
读取当前任务目的地
↓
生成 2～3 路线
↓
选择路线
↓
实时导航
↓
路口 maneuver
↓
故意偏航
↓
自动 reroute
↓
继续导航
↓
到达目的地
```

这才是 P2 真正关门条件。

---

# 179. P2 关门报告

最终创建：

```text
docs/validation/p2-closeout-2026-08.md
```

至少记录：

- Dataset v2；
- Map Matching；
-路由正确性；
-路线性能；
- alternatives；
- rerouting；
- maneuver；
-roundabout；
-signal linkage；
-完整游戏 session；
-性能基准；
-已知限制。

---

# 180. P2 关门 Tag

全部 Gate 通过后：

```text
v0.3.0-p2
```

P3 才正式开始。

---

# 181. P2 完成后的正式数据流

P2 关门后系统应形成：

```text
ETS2
 │
 │ Telemetry
 ▼
Navigation Core
 │
 ├─ Map Match
 │
 ├─ Route
 │
 ├─ Alternatives
 │
 ├─ Progress
 │
 ├─ Reroute
 │
 ├─ Maneuver
 │
 └─ Signal Association
 │
 ▼
NavigationSnapshot
```

P3 只需在这一稳定状态上实现：

```text
Overspeed
Speed Camera
Traffic Light Warning
Green Soon
GLOSA
Driving Assistant
```

P4 则直接将：

```text
NavigationSnapshot
+
DrivingAssistantSnapshot
```

渲染为最终高德式界面。

---

# 182. P2 的核心成功判据

P1 的成功判据是：

\[
\boxed{
\text{Official ETS2 Map}
\rightarrow
\text{Verified Navigation Dataset}
}
\]

P2 的成功判据是：

\[
\boxed{
\text{Telemetry + Destination + Dataset}
\rightarrow
\text{Stable Real-Time Navigation State}
}
\]

更具体地说，P2 成功不等于：

> A* 能在两点之间找到一条路。

而是：

> 游戏中的车辆能够被稳定定位在自主地图上，从任意当前位置规划合理路线，实时跟踪路线进度，产生正确转向指令，在偏航后重新规划，并把前方规划 movement 与真实信号灯建立关联。

---

# 183. 当前最优先执行项

从 `v0.2.0-p1` 开始，不建议立即创建：

```text
nav-routing/AStar.rs
```

然后直接开始性能测试。

正确顺序是：

```text
① Dataset Consumer Audit

② Dataset v2
   ├─ edge geometry
   ├─ movement geometry
   ├─ speed metadata
   └─ ferry/train connectivity

③ Rust Dataset Runtime
   └─ compact CSR

④ Telemetry Trace

⑤ UK / Speed / Roundabout 前置验证

⑥ Spatial Index

⑦ Map Matching

⑧ Dijkstra / A*

⑨ Profiles / Alternatives

⑩ Route Tracker / Rerouting

⑪ Maneuver / Roundabout

⑫ Signal Runtime Link

⑬ Full Navigation Session

⑭ Europe Regression / Performance
```

其中：

\[
\boxed{
\text{Dataset v2 Geometry}
}
\]

和：

\[
\boxed{
\text{Map Matching}
}
\]

是 P2 最先需要攻克的两个基础模块。

原因在于当前 P1 图已经足够支持路径搜索，但还不足以精确回答真正实时导航最基本的问题：

> **“车辆此刻究竟在哪一条路、哪个方向、距离该道路起点多远？”**

只有这个问题稳定解决，后面的偏航、剩余距离、转向距离和实时导航才具有可靠基础。