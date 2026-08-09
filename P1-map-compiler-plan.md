# ETS2Nav P1 — Map Compiler 完整化开发规划

**文档状态**：正式执行基线  
**阶段**：P1 — Map Compiler  
**前置版本**：`v0.1.0-p0`  
**适用项目**：HaowenCang/ETS2Nav  
**技术基线**：《Euro Truck Simulator 2 外部智能导航系统 v0.2》  
**阶段目标**：从 P0“可解析、可连通”的地图实验系统，升级为能够从 ETS2 + 官方 DLC 自动构建正式 Navigation Dataset 的生产级 Map Compiler。

---

# 1. P1 阶段定义

P1 的任务不是开发完整导航软件，也不是实现最终 A*、偏航重规划、驾驶辅助或正式 UI。

P1 只解决一个核心问题：

> 如何从用户本机安装的 ETS2 与官方地图 DLC 中，自动恢复足以支撑导航的地图语义，并生成稳定、可验证、可版本化、可由 P2 Navigation Core 直接读取的 Navigation Dataset。

完整转换链定义为：

```text
ETS2 / Official DLC
        │
        ▼
SCS Archives / Definitions / Map Sectors / Prefabs
        │
        ▼
Raw Parsed Model
        │
        ▼
Semantic Map Model
        │
        ├── Road semantics
        ├── Junction semantics
        ├── POI semantics
        ├── Speed/sign semantics
        ├── Semaphore bindings
        └── Ferry/train/service connections
        │
        ▼
Navigation Graph Model
        │
        ├── Routing Graph
        └── Junction Graph
        │
        ▼
Validation / Regression
        │
        ▼
Navigation Dataset
```

P1 完成后，所有 ETS2 专有地图格式知识应当封装在 Map Compiler 内。

P2 之后的模块不得再直接依赖：

- `.scs`；
- `.base`；
- `.aux`；
- `.sii`；
- `.ppd`；
- sector binary；
- SCS-specific UID resolution。

因此 P1 的核心架构原则是：

\[
\boxed{
\text{SCS format complexity ends at P1}
}
\]

---

# 2. P0 已完成基础

P1 不重复实现以下已经通过 P0 验证的内容。

## 2.1 HashFS

已经具备：

- HashFS v1/v2 读取；
- archive 内容枚举；
- 游戏真实资源读取；
- 与成熟实现进行差分验证的能力。

P0 已使用真实 ETS2 数据完成大规模测试。

---

## 2.2 Sector Binary

已经能够解析：

- sector；
- item；
- node；
- road；
- prefab；
- 其他 P0 已覆盖 item 类型。

P0 对 282 个 sector 与 TruckLib 进行逐项对照，结果为：

```text
174735 items
248410 nodes
差异：0
```

因此 P1 不再重新设计 sector 基础 reader。

---

# 3. 当前 P0 图模型的边界

当前 `RoadGraph` 只适合作为 P0 连通性验证。

现有行为包括：

```text
Road:
A ↔ B
```

即所有 road 都无条件生成两个方向。

同时对于一个 prefab：

```text
A
├── B
├── C
└── D
```

当前近似为：

```text
A ↔ B
A ↔ C
A ↔ D
B ↔ C
B ↔ D
C ↔ D
```

即 prefab connector 全连接。

这种模型对于验证：

> “地图是否能够大规模自动连接”

是合理的。

但它不能作为正式导航路网，因为会产生：

- 逆行；
- 不存在的转向；
- 非法 U-turn；
- 高速互通错误连接；
- 环岛错误 movement；
- 收费站错误连接；
- 公司入口错误连接。

因此：

\[
\boxed{
\text{P1 首要任务不是扩大地图规模，而是消除 P0 拓扑近似}
}
\]

P1 Gate 不允许正式 `routing.graph` 中继续使用：

```text
所有道路双向
```

或：

```text
prefab connector 全连接
```

作为一般规则。

---

# 4. P1 总体出口

P1 完成后，用户应能够执行一次命令：

```text
ets2nav-map compile
```

输入：

```text
Euro Truck Simulator 2 安装目录
```

自动完成：

```text
ETS2 安装检测
        ↓
官方 DLC 检测
        ↓
资源优先级恢复
        ↓
地图数据解析
        ↓
定义解析
        ↓
Prefab 导航语义恢复
        ↓
正式有向路网构建
        ↓
Junction Graph 构建
        ↓
POI 解析
        ↓
限速 / sign / semaphore metadata
        ↓
地图瓦片生成
        ↓
Graph Validation
        ↓
Regression Validation
        ↓
Navigation Dataset
```

最终产生：

```text
navigation-data/
│
├─ manifest.json
├─ map.db
├─ routing.graph
├─ junction.graph
├─ search.db
├─ map.pmtiles
└─ diagnostics.json
```

整个流程不要求：

- QGIS；
- SCS Editor；
- 用户人工修改道路；
- 手工编辑 graph；
- 手工指定官方 DLC。

---

# 5. P1 正式范围

P1 必须完成：

- 官方基础地图识别；
- 官方 DLC 自动识别；
- archive overlay/resource resolution；
- 地图 definition 解析；
- road semantics；
- prefab descriptor/navigation semantics；
- 正式 directed routing graph；
- junction graph；
- graph validation；
- POI；
- search index；
- road/speed/sign metadata；
- semaphore static binding；
- ferry/train connection；
- vector tiles；
- Navigation Dataset；
- deterministic build；
- 全欧洲官方地图构建；
- regression corpus；
- P1 CLI；
-开发诊断工具。

---

# 6. P1 非目标

P1 明确不实现：

- 正式 Navigation Core；
- 正式 A* 优化；
- ALT；
- CH/CCH；
- 多路线策略；
- Map Matching；
- 偏航检测；
- 偏航重规划；
- maneuver generator；
- 红灯减速提示；
- GLOSA；
- 超速逻辑；
- TTS；
- 正式高德式 UI；
- Android/iOS 客户端；
- TruckersMP；
- ProMods 专用适配；
- 第三方地图兼容保证；
- 增量地图构建；
- 实时道路事件；
- 车道级导航 UI。

P1 可以包含简单 Dijkstra/A* 调试查询，但只能用于验证图是否正确。

---

# 7. P1 技术架构

P1 Map Compiler 推荐分为五层。

```text
Layer 1
Physical Resources
        │
        ▼
Layer 2
SCS Format Model
        │
        ▼
Layer 3
Semantic Map Model
        │
        ▼
Layer 4
Navigation Graph Model
        │
        ▼
Layer 5
Dataset / Tiles / Diagnostics
```

---

# 8. Layer 1 — Physical Resource Layer

负责：

> 游戏文件在哪里，以及 ETS2 最终看到哪个资源。

包括：

- base archive；
- def archive；
- map archive；
- 官方 DLC；
- loose files；
-未来实验性 Mod overlay。

这一层不得理解道路或路口语义。

---

# 9. Resource Resolver

新增模块：

```text
map-compiler/src/ScsResource/
```

定义统一资源接口：

```csharp
public interface IScsResourceProvider
{
    bool Exists(string virtualPath);

    Stream Open(string virtualPath);

    IEnumerable<string> Enumerate(string virtualDirectory);
}
```

实现：

```text
HashFsProvider
DirectoryProvider
OverlayProvider
```

最终所有上层代码只看到统一：

```text
Virtual SCS Filesystem
```

例如：

```text
/def/world/semaphore_profile.sii
/map/europe/...
/prefab/...
```

而不关心该文件来自：

```text
base.scs
def.scs
base_map.scs
dlc_xxx.scs
```

---

# 10. Archive Overlay

必须恢复与 ETS2 实际加载逻辑一致的资源覆盖关系。

例如：

```text
base resource
    ↓
official DLC override
    ↓
higher-priority official resource
```

最终只能产生一个：

```text
ResolvedResource(path)
```

资源冲突必须能够记录：

```text
source archive
overridden archive
effective resource
```

便于诊断。

如果资源覆盖顺序错误，会造成：

- prefab 定义错误；
- road look 错误；
- semaphore profile 错误；
- DLC 地图解析与游戏实际不一致。

因此 Resource Resolver 属于 P1 blocking module。

---

# 11. Dataset Fingerprint

Resource Layer 同时生成：

```text
contentFingerprint
```

建议输入包括：

```text
game executable version
archive list
archive sizes
archive timestamps
relevant archive hashes
enabled official DLC
compiler schema version
```

最终：

```text
SHA-256
```

写入：

```text
manifest.json
```

用于判断：

> 当前 Navigation Dataset 是否仍对应用户的游戏版本。

---

# 12. Layer 2 — SCS Format Model

现有模块：

```text
ScsHashFs
ScsSector
ScsSii
```

继续作为：

> SCS 文件格式层。

这一层负责准确还原原始数据。

不得承担：

- “这是高速公路”；
- “这个路口允许左转”；
- “这里是加油站”；

等业务语义。

---

# 13. SII Parser 完整化原则

P1 不追求实现任意未知 SII 方言。

采用：

> corpus-driven parser。

流程：

```text
扫描官方 def corpus
        ↓
收集实际语法结构
        ↓
测试覆盖
        ↓
按需求扩充 parser
```

需要重点检查：

- `@include`；
- array；
- indexed array；
- token；
- local unit；
- tuple；
- quoted string；
- inheritance-like references；
- definition override；
- multiline/value edge cases。

---

# 14. Definition Resolver

新增：

```text
ScsDefinitions/
```

负责：

```text
raw SiiUnit
        ↓
resolved semantic definition
```

例如：

```text
RoadLookDefinition
CountryDefinition
CityDefinition
CompanyDefinition
PrefabDefinition
SemaphoreProfileDefinition
SignDefinition
FerryDefinition
```

不能让 Graph Builder 自己在任意位置查询：

```text
SiiUnit.Attributes
```

正式流程必须是：

```text
SII
↓
Definition Resolver
↓
Strongly Typed Model
↓
Semantic Map
```

---

# 15. Road Look

`RoadLookDefinition` 是 P1 最优先 definition。

必须恢复尽可能完整的：

- lane configuration；
- traffic direction；
- road class；
- shoulder；
- median；
- AI accessibility；
- lane count；
- road type；
-相关 routing metadata。

P1 的目标之一就是最终能够判断：

```text
Road A:
允许 A → B
不允许 B → A
```

或：

```text
A ↔ B
```

而不是一律双向。

---

# 16. Country / Road Rule

建立：

```text
CountryDefinition
```

为未来：

- 默认限速；
- 城市限速；
- truck speed rule；
-道路规则；

提供基础。

P1 不需要在这一阶段实现完整驾驶规则引擎，但 Map Dataset 必须保存这些原始/解析语义。

---

# 17. Layer 3 — Semantic Map Model

新增：

```text
ScsMapModel/
```

这是 P1 中最重要的新抽象层之一。

目标是把：

```text
SCS 格式对象
```

转换为：

```text
ETS2Nav 导航语义对象。
```

---

# 18. Semantic Map Model 建议结构

```text
SemanticMap
│
├─ Roads
├─ Junctions
├─ Cities
├─ Countries
├─ Companies
├─ Services
├─ Ferries
├─ Trains
├─ Signs
├─ SpeedRules
├─ Semaphores
└─ POIs
```

---

# 19. Semantic RoadSegment

正式：

```text
RoadSegment
```

至少包括：

```text
uid
node_start
node_end

geometry
length

road_look_id
road_class

forward_allowed
backward_allowed

lane_count_forward
lane_count_backward

country_id
city_id

speed_rule_refs

source_sector
source_item
```

注意：

```text
forward_allowed
backward_allowed
```

必须来自真实地图语义。

不得使用 P0 默认：

```text
true
true
```

作为生产行为。

---

# 20. Prefab Descriptor Parser

新增：

```text
ScsPrefab/
```

这是整个 P1 技术风险最高的模块。

负责读取：

- prefab descriptor；
- prefab node；
- curve；
- AI/navigation path；
- traffic lane；
- movement；
- priority；
- blinker；
- semaphore linkage；
- connection geometry。

建议数据模型：

```text
PrefabDescriptor
│
├─ Connectors
├─ NavigationLanes
├─ Movements
└─ SemaphoreBindings
```

---

# 21. JunctionMovement

P1 的核心抽象：

```text
JunctionMovement
```

例如：

```text
movement_id
prefab_id

entry_connector
exit_connector

allowed_vehicle_class
direction

turn_type

geometry
length

priority

semaphore_id
```

这样正式路口：

```text
A → C
```

只有在 prefab navigation 数据明确允许时才存在。

---

# 22. Prefab 全连接必须消失

P1 正式图禁止：

```text
foreach connector A
foreach connector B
AddEdge(A, B)
```

正式规则改为：

```text
foreach NavigationMovement movement
AddMovement(
    movement.Entry,
    movement.Exit
)
```

因此：

\[
\boxed{
\text{Prefab connectivity derives from navigation semantics}
}
\]

而不是 connector 几何邻接。

---

# 23. 环岛

P1 必须使环岛在图层上保持正确拓扑：

- 合法进入；
- 合法绕行方向；
- 合法出口；
- 禁止逆向；
- 禁止不可能跨越。

P1 不负责输出：

> “第三出口驶出”

这一 maneuver 文本。

但 P1 的图必须包含足够语义，让 P2 能可靠计算第几个出口。

---

# 24. 高速互通

重点检查：

```text
motorway mainline
ramp
merging lane
exit ramp
```

必须避免：

- 主路直接跨越 ramp；
- ramp 反向通行；
- 两条物理接近但无连接的高架道路被连接。

---

# 25. 公司入口

公司不得表示为：

```text
POI center
```

而应有：

```text
company
    │
    ├─ visual position
    └─ routing access
```

其中：

```text
routing access node
```

必须位于合法道路/园区入口。

这是 P2 当前任务目的地导航所必需。

---

# 26. Ferry / Train

这些连接不是普通 Road Edge。

正式 Graph 使用：

```text
EdgeKind.Ferry
EdgeKind.Train
```

至少保存：

```text
origin
destination
distance
travel-time metadata
cost metadata
```

P2 将来决定：

> 是否通过 ferry/train。

P1 只恢复连接。

---

# 27. Layer 4 — Navigation Graph Model

P1 建议继续采用：

> Routing Graph + Junction Graph。

不能合并为一个超细粒度图。

---

# 28. Routing Graph

用于：

- 城市间；
- 国家间；
- 欧洲长距离；

路径规划。

节点尽量保持宏观。

正式 edge 类型：

```text
Road
JunctionMovement
Ferry
Train
ServiceAccess
```

---

# 29. Routing Edge

建议结构：

```text
RoutingEdge
{
    edge_id

    from
    to

    kind

    source_uid

    length

    geometry_offset
    geometry_count

    road_class

    road_metadata_id

    country_id
    city_id

    junction_id
    movement_id

    speed_profile_id

    semaphore_binding_id

    flags
}
```

---

# 30. Junction Graph

用于：

- 精细路口 topology；
- signal mapping；
- Junction View；
-未来 lane-level guidance。

保存：

```text
junction
connector
navigation_lane
movement
movement geometry
semaphore binding
```

P2 Routing 不必遍历所有 lane-level node。

这样避免：

\[
|V_\mathrm{route}|
\]

因 prefab 内部细节急剧膨胀。

---

# 31. Routing Graph 与 Junction Graph 关系

通过：

```text
junction_id
movement_id
```

关联。

Routing Graph 中：

```text
A → JunctionMovement#427 → B
```

Junction Graph 提供：

```text
Movement#427:
entry
exit
geometry
lane
signal
```

---

# 32. 正式 Graph Builder

建议新增：

```text
RoutingGraphBuilder
JunctionGraphBuilder
```

禁止继续让：

```text
RoadGraph.Build(IEnumerable<SectorFile>)
```

直接成为生产入口。

正式链路：

```text
SectorFile
       ↓
SemanticMapBuilder
       ↓
SemanticMap
       ↓
RoutingGraphBuilder
       ↓
RoutingGraph
```

---

# 33. P1 Graph Validation

P0 `GraphValidator` 已有：

- self-loop；
- duplicate edge；
- broken node ref；
- direction mismatch；
- dead-end；

等基础检查。

P1 将其拆分正式化。

新增：

```text
ScsValidation/
```

---

# 34. Validation 架构

```text
ValidationEngine
│
├─ StructuralValidator
├─ ReferenceValidator
├─ DirectionValidator
├─ ConnectivityValidator
├─ JunctionValidator
├─ GeometryValidator
├─ SemanticValidator
├─ PoiValidator
└─ DatasetValidator
```

---

# 35. Severity

所有问题必须分级：

```text
Fatal
Error
Warning
Info
```

### Fatal

说明 dataset 无法安全使用。

例如：

- graph 文件损坏；
- node reference 越界；
-全局关键定义缺失。

### Error

明确存在导航错误。

例如：

- illegal direction；
-不存在的 movement；
- geometry 断裂。

### Warning

可疑但未证明错误。

例如：

- 小 isolated component；
- unusual turn angle；
-无法匹配 POI entrance。

### Info

统计和辅助诊断。

---

# 36. Structural Validation

至少检测：

- self-loop；
- duplicate edge；
- duplicate UID；
- missing node；
- broken edge reference；
- invalid geometry index；
-无效 offset/count。

---

# 37. Direction Validation

至少检测：

- one-way contradiction；
- reverse movement；
- road definition 与 graph direction 不一致；
- prefab movement 方向矛盾；
- ferry/train 方向异常。

---

# 38. Junction Validation

至少检测：

- connector 无入口；
- connector 无出口；
- movement 引用不存在 connector；
-非法 U-turn；
-缺失 movement；
-一个 movement 对应错误 prefab；
- semaphore binding 引用不存在 group。

---

# 39. Geometry Validation

检测：

\[
d(p_\mathrm{edge,end},p_\mathrm{next,start})
\]

是否在合理容差内。

同时检测：

-异常 teleport；
-极端 heading jump；
- road geometry NaN；
- zero-length edge。

---

# 40. Connectivity Validation

计算：

- connected components；
- largest component；
- component size distribution；
- isolated road；
- isolated POI；
- unreachable company；
- ferry-only island；
- DLC boundary connectivity。

注意：

> component 数量不要求为 1。

因为：

- 岛屿；
- 独立 ferry destination；
- 地图结构；

可能合法形成多个 component。

必须结合语义判断。

---

# 41. Semantic Validation

这是 P1 与 P0 最大区别。

典型检查：

```text
road进入prefab
但最终movement出口与navigation path不符
```

或：

```text
motorway mainline
直接连接反向出口
```

这些错误从纯 graph theory 看可能完全合法。

因此必须引入：

```text
Semantic Corpus
```

验证真实导航语义。

---

# 42. Diagnostics Output

生成：

```text
diagnostics.json
```

示例：

```json
{
  "fatal": 0,
  "errors": 0,
  "warnings": 37,
  "stats": {
    "nodes": 123456,
    "edges": 234567,
    "junctions": 18492
  }
}
```

每条问题记录：

```text
code
severity
source UID
sector
coordinates
description
```

便于 map-inspector 定位。

---

# 43. P1 Developer Tools

P0 遗留 A8 在 P1 前期必须完成。

至少实现：

```text
map-inspector
graph-debugger
```

---

# 44. map-inspector

允许按：

- UID；
- coordinate；
- sector；
- prefab；
- road；

查询。

显示：

```text
item type
position
source sector
road look
definition
connected edges
junction
movement
semaphore
POI
```

---

# 45. graph-debugger

建议使用：

```text
localhost Web App
+
MapLibre
+
GeoJSON debug export
```

支持：

- 点选起点；
- 点选终点；
- Dijkstra path；
- 显示 node；
-显示 directed edge；
-显示 edge kind；
-显示 movement；
-显示 semaphore；
-显示 component；
-显示 error/warning marker。

---

# 46. Debug Route

P1 可以使用：

```text
Dijkstra
```

因为此时目的不是性能。

目的只是确认：

> 图上的合法路线是否存在。

因此不要在 P1 花时间优化 A* heuristic。

---

# 47. POI Model

P1 必须生成正式 POI 数据。

基础类型：

```text
City
Company
Garage
Repair
Fuel
Rest
Ferry
Train
Toll
Border
```

---

# 48. POI 数据结构

```text
Poi
{
    poi_id
    type

    internal_token
    display_name

    position

    city_id
    country_id

    access_node
    access_edge

    metadata
}
```

必须区分：

```text
visual position
```

和：

```text
routing access position
```

---

# 49. 公司

至少保存：

```text
company token
company display name
city
position
access
```

以支持未来：

```text
Telemetry job destination
          ↓
destination company ID
          ↓
POI lookup
          ↓
routing target
```

---

# 50. 加油与休息

P1 只构建地图实体。

不判断：

> 当前油量是否应该加油。

需要保存：

```text
fuel POI
rest POI
service area
access node
```

P3/P2 再做沿途规划。

---

# 51. Search Database

建议：

```text
SQLite + FTS5
```

输出：

```text
search.db
```

而不是自研搜索索引格式。

基本表：

```text
country
city
poi
alias
fts_index
```

搜索对象：

- city；
- company；
- garage；
- repair；
- fuel；
- rest；
- ferry；
- train。

---

# 52. Localization

P1 只保存：

```text
internal name
resolved display string
localization key
```

中文别名可以后续增加。

不要在 P1 手动维护：

```text
Berlin → 柏林
```

这种翻译数据库。

---

# 53. Speed Model

P1 必须建立能支持未来：

> 当前限速 + 前方限速

的数据模型。

不能只保存：

```text
edge.speedLimit
```

因为同一 edge 内可能发生限速变化。

建议：

```text
SpeedProfile
```

---

# 54. SpeedProfile

例如：

```text
Edge #13872

0 m ───────── 320 m
80 km/h

320 m ─────── 915 m
50 km/h
```

表示为：

```text
SpeedSegment
{
    start_offset
    end_offset
    limit
    source
}
```

---

# 55. Speed Rule 数据来源

依次研究：

```text
country default
road look
road class
sign
city rule
map-specific rule
```

P1 不应假设某一个来源可以覆盖所有情况。

最终使用真实 Telemetry 当前 speed limit 做 validation oracle。

---

# 56. Telemetry Speed Ground Truth

建立 P1 验证工具：

```text
vehicle position
       ↓
Map Match / nearest debug edge
       ↓
Map-derived speed
       ↕
Telemetry speed limit
```

记录：

```text
coordinate
edge
map limit
telemetry limit
difference
```

这里的 Map Matching 可以使用简单局部匹配，不属于正式 P2 Map Matching。

目的仅为：

> 验证限速解析。

---

# 57. Sign Parser

P1 应建立：

```text
SignMetadata
```

优先解析与导航直接相关内容：

- speed limit；
- road number；
- exit information；
- direction information；
-其他必要 routing metadata。

不要求完整解析所有装饰性 sign。

---

# 58. Speed Camera

P1 的目标不是承诺所有测速点均能识别。

P1 只进行：

> 静态测速设施数据源确认。

输出候选：

```text
SpeedCamera
{
    position
    direction
    road_edge
    speed_limit
    confidence
    source
}
```

只有可靠性达到要求的对象才写入正式 dataset。

---

# 59. Semaphore 在 P1 的职责

P0 已经解决：

> 当前信号灯运行时状态如何获得。

P1 不再重新研究：

- clock domain；
- phase anchor；
- countdown extrapolation。

P1 只负责静态关联：

\[
\boxed{
\text{Junction Movement}
\leftrightarrow
\text{Semaphore Group/Profile}
}
\]

---

# 60. Semaphore Static Model

保存：

```text
SemaphoreBinding
{
    junction_id
    movement_id

    prefab_semaphore_id

    profile_id

    cycle_group
}
```

这样运行时 Navigation Core 可根据：

```text
当前 route movement
```

找到：

```text
当前应该关注的信号灯。
```

---

# 61. P0 Runtime Signal Bridge

P1 不修改其核心设计。

它继续作为：

> Runtime signal state source。

但 P1 Dataset 需要为未来建立：

```text
map semaphore
        ↔
runtime semaphore object
```

所需的位置和静态 metadata。

真正 runtime association 可在 P2/P3 完成。

---

# 62. Navigation Dataset

P1 必须定义稳定的：

```text
ETS2NAV_DATASET_VERSION
```

初始：

```text
1
```

P2 只读取这一格式。

---

# 63. manifest.json

至少：

```json
{
  "datasetVersion": 1,
  "compilerVersion": "...",
  "gameVersion": "...",
  "map": "europe",
  "fingerprint": "...",
  "officialDlc": [],
  "counts": {
    "nodes": 0,
    "edges": 0,
    "junctions": 0,
    "pois": 0
  },
  "validation": {
    "fatal": 0,
    "errors": 0,
    "warnings": 0
  }
}
```

---

# 64. map.db

推荐：

```text
SQLite
```

保存：

```text
metadata
country
city
poi
road_metadata
speed_profile
speed_segment
sign
semaphore_profile
ferry
train
```

SQLite 不承担高频 Routing Graph 遍历。

---

# 65. routing.graph

必须使用自定义紧凑二进制，而不是：

- JSON；
- .NET BinaryFormatter；
- protobuf 对象树。

建议布局：

```text
Header
Node Table
Edge Table
Geometry Table
Attribute Table
```

---

# 66. Binary Format 原则

显式定义：

```text
magic
version
endianness
node count
edge count
offset table
coordinate representation
integer width
float width
```

例如：

```text
Magic = ETS2NRG1
```

读取方必须校验：

```text
magic
version
file size
checksum
```

---

# 67. junction.graph

保存：

```text
junction
connector
lane
movement
geometry
semaphore binding
```

可采用：

- 自定义 binary；
- FlatBuffers；
-其他明确跨语言格式。

但不得依赖 .NET runtime serialization。

---

# 68. Rust 兼容

因为 P2 Navigation Core 计划使用 Rust：

P1 Dataset 必须确保：

```text
C# compiler
    ↓
stable bytes
    ↓
Rust reader
```

建议在 P1 后期提前写：

```text
dataset-reader-smoke
```

一个极小 Rust 程序，只验证：

- header；
- node count；
- edge count；
- geometry。

不是开始 P2。

目的是提前证明跨语言接口没有设计缺陷。

---

# 69. Map Tiles

输出：

```text
map.pmtiles
```

供未来 MapLibre 使用。

P1 只负责几何正确。

不负责最终高德风格。

---

# 70. Vector Layers

正式图层：

```text
road
junction
city
company
service
fuel
rest
ferry
train
toll
border
```

开发模式增加：

```text
routing_node
routing_edge
junction_movement
semaphore
validation_error
```

---

# 71. Tiles 与 Routing 必须解耦

Map tiles 只是视觉表现。

Routing Graph 是导航逻辑。

禁止：

```text
从 PMTiles 恢复路线
```

或：

```text
为了渲染简化而修改 routing geometry。
```

---

# 72. Deterministic Build

相同：

```text
game files
compiler version
configuration
```

必须生成相同：

```text
routing.graph
junction.graph
map.db semantic content
```

即：

\[
F(X)=Y
\]

不得因为：

- Dictionary iteration；
- thread scheduling；
-随机 UID ordering；

造成输出变化。

---

# 73. Deterministic ID

内部连续 ID 必须由稳定排序生成。

例如：

```text
source UID
source type
```

排序后分配：

```text
node_id = 0...N-1
```

而不是依赖：

```text
Dictionary enumeration order
```

---

# 74. Build Manifest 可变字段

例如：

```text
buildTimestamp
```

允许变化。

但不能进入：

```text
semanticHash
```

因此可同时保存：

```text
fileHash
semanticHash
```

---

# 75. P1 Regression Strategy

P1 必须建立三类 regression corpus。

---

# 76. Parser Corpus

保存真实：

- sector samples；
- SII samples；
- prefab descriptor samples；
- definition samples。

覆盖：

- base；
-不同官方 DLC；
-不同年代地图区域。

---

# 77. Semantic Corpus

人工确认若干典型路口。

至少包括：

```text
标准十字路口
T 字路口
多车道路口
环岛
高速入口
高速出口
复杂互通
收费站
边检
公司入口
加油站入口
```

每个 fixture 定义：

```text
allowed movements
forbidden movements
expected direction
expected semaphore binding
```

---

# 78. Route Corpus

固定 origin/destination。

定义：

```text
origin
destination

required edge/area
forbidden maneuver
expected connectivity
```

P1 不验证：

> 路线是否最快。

只验证：

> 路线是否合法。

---

# 79. Random OD

Berlin semantic graph 完成后：

```text
seed fixed
200–500 OD
```

Germany：

```text
≥500
```

Europe：

```text
≥1000
```

检查：

- route exists；
- geometry continuous；
-合法 edge；
-合法 junction movement；
-无 illegal U-turn；
-无 reverse edge。

---

# 80. P1 扩展地图顺序

禁止：

```text
Berlin
↓
Entire Europe
```

推荐：

```text
Berlin Core
     ↓
Berlin + Surroundings
     ↓
Berlin Region
     ↓
Germany
     ↓
Base Europe
     ↓
All Installed Official DLC
```

每一级都建立 baseline。

---

# 81. Scale Baseline

每次记录：

```text
sector count
road count
prefab count
node count
edge count
junction count
component count
largest component ratio
POI count
compile time
peak memory
warnings
errors
```

例如若：

```text
Germany
component = 34
```

扩展 Europe 后：

```text
component = 10000
```

应当立即视为 parser/overlay/graph regression。

---

# 82. TruckLib 使用策略

P0 已经证明 TruckLib 对 differential validation 很有价值。

P1 保留：

```text
ETS2Nav Parser
        │
        ▼
Normalized Output
        │
        ↕ diff
        │
TruckLib
```

但默认不把 TruckLib 作为 production dependency。

原因包括：

- 项目许可证标识为 GPL v2；
- ETS2Nav 当前为 GPL-3.0；
-在未确认许可证兼容方式之前，不应直接组合发行；
- TruckLib 本身仍明确描述为 alpha；
- prefab 本身也属于其已知风险区域。

因此 P1 采用：

\[
\boxed{
\text{TruckLib = Oracle / Differential Test Tool}
}
\]

而不是：

\[
\boxed{
\text{TruckLib = Map Compiler Runtime Dependency}
}
\]

若未来确认获得适当许可或重新设计发行边界，再重新评估。

---

# 83. 外部项目的使用原则

允许研究：

- TruckLib；
- TruckSim Maps；
- ETS2LA；
-其他 GPL 项目。

但任何代码复用都应：

- 记录来源；
-记录许可证；
-保留版权信息；
-明确复用模块；
-符合项目 GPL-3.0 及上游许可证要求。

建议新增：

```text
docs/third-party/
```

记录所有复用或参考情况。

---

# 84. 推荐工程目录

P1 结束时建议：

```text
map-compiler/
│
├─ MapCompiler.sln
│
├─ src/
│   ├─ ScsHashFs/
│   ├─ ScsResource/
│   ├─ ScsSector/
│   ├─ ScsSii/
│   ├─ ScsDefinitions/
│   ├─ ScsPrefab/
│   ├─ ScsMapModel/
│   ├─ ScsGraph/
│   ├─ ScsValidation/
│   ├─ ScsPoi/
│   ├─ ScsTiles/
│   ├─ ScsDataset/
│   └─ MapCompiler.Cli/
│
└─ tests/
    ├─ ScsHashFs.Tests/
    ├─ ScsResource.Tests/
    ├─ ScsSector.Tests/
    ├─ ScsSii.Tests/
    ├─ ScsDefinitions.Tests/
    ├─ ScsPrefab.Tests/
    ├─ ScsMapModel.Tests/
    ├─ ScsGraph.Tests/
    ├─ ScsValidation.Tests/
    ├─ ScsDataset.Tests/
    └─ MapCompiler.IntegrationTests/
```

---

# 85. Developer Tools 目录

```text
tools/
├─ map-inspector/
├─ graph-debugger/
├─ dataset-inspector/
├─ speed-validator/
└─ p1-regression/
```

P0 原有：

```text
telemetry-dump
signal-lab
sem-probe
...
```

继续保留，不纳入 Map Compiler 本体。

---

# 86. MapCompiler CLI

正式命令：

```text
ets2nav-map
```

---

# 87. compile

```text
ets2nav-map compile
    --game-dir <path>
    --map europe
    --output <path>
```

功能：

> 完整构建 Navigation Dataset。

---

# 88. validate

```text
ets2nav-map validate <dataset>
```

只执行：

- graph；
- semantic；
- dataset；

验证。

---

# 89. inspect

```text
ets2nav-map inspect
    --uid <uid>
```

或者：

```text
--coord x,z
--sector ...
--prefab ...
```

输出开发信息。

---

# 90. fingerprint

```text
ets2nav-map fingerprint
```

输出：

```text
game version
DLC
archives
fingerprint
dataset compatibility
```

---

# 91. dump

调试导出：

```text
GeoJSON
JSON
CSV
```

不得作为正式 Dataset API。

---

# 92. P1 工作包

正式划分如下。

---

# 93. P1-00 — Baseline Freeze

内容：

- 更新 README 当前阶段；
-更新 PLAN；
-冻结 P0 fixtures；
-记录 `v0.1.0-p0`；
-整理 P0 遗留 A6/A8；
-确定 TruckLib 只作为 oracle；
-新增 P1 architecture decision。

完成条件：

```text
P1 baseline documented
```

---

# 94. P1-01 — Validation & Debug Tooling

内容：

- GraphValidator 拆分；
- severity；
- diagnostics format；
- map-inspector；
- graph-debugger。

原因：

必须先拥有能够“看到错误”的工具，再重写 topology。

完成条件：

> Berlin P0 graph 可以完整可视化，并能显示 validation error。

---

# 95. P1-02 — Resource Resolver

内容：

- IScsResourceProvider；
- HashFsProvider；
- DirectoryProvider；
- OverlayProvider；
- DLC detection；
- fingerprint。

完成条件：

> 所有上层模块只能通过 virtual resource API 读取地图资源。

---

# 96. P1-03 — Definition Layer

内容：

- SII corpus；
- include；
- definition resolver；
- road look；
- country；
- city；
- company；
- semaphore；
- ferry/train；
- sign 基础。

完成条件：

> Berlin 需要的所有 road/prefab definition 均解析为 typed model。

---

# 97. P1-04 — Prefab Navigation Parser

内容：

- prefab descriptor；
- connector；
- curves；
- navigation lanes；
- movement；
- signal ID。

完成条件：

> 至少对 semantic corpus 中全部典型 prefab 恢复合法 movement。

这是 P1 最关键单项工作。

---

# 98. P1-05 — Semantic Graph

内容：

- SemanticMap；
- RoadSegment；
- Junction；
- JunctionMovement；
- RoutingGraphBuilder；
- JunctionGraphBuilder。

同时删除：

```text
road unconditional bidirectional
prefab full-connect
```

完成条件：

> Berlin 正式 semantic graph 可以生成。

---

# 99. P1-06 — Berlin Semantic Gate

这是一个独立 Gate。

必须验证：

-十字路口；
-环岛；
-高速入口；
-高速出口；
-company；
-fuel；
-service。

运行：

```text
≥500 deterministic random OD
```

要求：

```text
0 fatal
0 known illegal movement
0 known reverse routing
```

如果 Berlin Gate 不通过：

> 不得进入 Germany。

---

# 100. P1-07 — Germany Scale Test

将正式 semantic graph 扩至德国。

重点暴露：

-不同 prefab family；
-不同 road look；
-高速复杂度；
-城市/乡村差异。

完成条件：

```text
完整 Germany build
validation 无 blocker
random OD ≥500
```

---

# 101. P1-08 — POI / Search

完成：

- city；
- company；
- garage；
- repair；
- fuel；
- rest；
- ferry；
- train；
- toll；
- border。

并生成：

```text
search.db
```

完成条件：

> POI 均拥有有效 routing access 或明确标为非 routing POI。

---

# 102. P1-09 — Road Rules / Signs / Speed

完成：

- speed model；
- sign parser；
- speed segments；
- Telemetry validation recorder；
- camera candidate 调研。

完成条件：

> 代表性路线中 map speed 与 telemetry speed limit 高一致率。

具体容差/覆盖率应由实测数据确定，不预先制造未经验证的百分比要求。

---

# 103. P1-10 — Semaphore Binding

完成：

```text
JunctionMovement
       ↕
SemaphoreProfile / Semaphore ID
```

完成条件：

> P0 已测试信号灯路口均能确定规划 movement 所受的 signal group。

---

# 104. P1-11 — Dataset Writer

完成：

```text
manifest.json
map.db
routing.graph
junction.graph
search.db
diagnostics.json
```

以及：

```text
Rust dataset-reader-smoke
```

完成条件：

> Dataset 可脱离 C# runtime 被独立读取。

---

# 105. P1-12 — Vector Tiles

生成：

```text
map.pmtiles
```

完成：

- road；
- city；
- POI；
- developer layers。

完成条件：

> graph-debugger/MapLibre 可以直接加载 dataset 地图。

---

# 106. P1-13 — Europe Build

范围：

> 用户当前安装的官方 base + 全部官方 DLC。

完成条件：

```text
complete build
0 fatal
0 unresolved parser error
```

不能因为某些 DLC 不理解而静默跳过。

---

# 107. P1-14 — Regression Suite

整合：

- parser corpus；
- semantic corpus；
- route corpus；
- random OD；
- determinism；
- dataset reader；
- scale regression。

完成条件：

```text
one command
→ complete P1 test suite
```

---

# 108. P1 工作依赖关系

```text
P1-00
  │
  ▼
P1-01
  │
  ├────────────┐
  ▼            ▼
P1-02        Debug Tools
  │
  ▼
P1-03
  │
  ▼
P1-04
  │
  ▼
P1-05
  │
  ▼
Berlin Gate
  │
  ▼
Germany
  │
  ├────── P1-08 POI
  ├────── P1-09 Rules
  └────── P1-10 Semaphore
            │
            ▼
        P1-11 Dataset
            │
            ▼
        P1-12 Tiles
            │
            ▼
        P1-13 Europe
            │
            ▼
        P1-14 Regression
```

---

# 109. 推荐开发优先级

最高：

```text
Resource semantics
Road semantics
Prefab semantics
Semantic graph
Validation
```

中：

```text
POI
speed
sign
semaphore binding
dataset
```

后：

```text
tiles
full Europe
```

绝对不应先做：

```text
漂亮地图
全欧洲扫描
POI UI
```

再回来解决 topology。

---

# 110. Branch Strategy

P1 起正式采用：

```text
main
+
feat/<task>
```

---

# 111. 推荐 PR 顺序

```text
feat/p1-00-baseline
feat/p1-01-validation-tools
feat/p1-02-resource-resolver
feat/p1-03-definition-model
feat/p1-04-prefab-parser
feat/p1-05-semantic-map
feat/p1-06-semantic-graph
feat/p1-07-berlin-gate
feat/p1-08-germany-scale
feat/p1-09-poi-search
feat/p1-10-road-rules
feat/p1-11-semaphore-binding
feat/p1-12-dataset
feat/p1-13-vector-tiles
feat/p1-14-europe-build
feat/p1-15-regression-suite
```

不要将：

```text
prefab parser
+
semantic graph rewrite
+
Europe build
```

放入一个 PR。

---

# 112. PR 基本验收

每个 PR 必须：

```text
dotnet build
dotnet test
```

通过。

涉及格式的 PR：

> 必须增加真实 fixture。

涉及 graph 的 PR：

> 必须增加 semantic regression。

涉及 dataset 的 PR：

> 必须增加 reader compatibility test。

---

# 113. 测试层级

### Unit Test

单个 parser/model。

### Fixture Test

真实 SCS 数据。

### Differential Test

ETS2Nav vs TruckLib / 其他 oracle。

### Semantic Test

已知路口 movement。

### Integration Test

区域完整 build。

### Scale Test

Germany/Europe。

### Runtime Validation

Telemetry speed/sign 等。

---

# 114. CI

P1 建议建立 GitHub Actions：

```text
build
unit-test
fixture-test
```

不建议 CI 中运行完整 Europe build，因为：

- 游戏文件无法合法直接入库；
-体积大；
-耗时高。

Europe integration 保留本地测试。

CI 使用：

```text
small fixtures
```

---

# 115. Fixtures 与版权

不得提交大规模 ETS2 原始游戏资源。

测试 fixture 应控制为：

-必要最小片段；
-或由脚本从用户本地游戏提取；
-或使用可以合法分发的自造数据。

仓库不应成为游戏资源镜像。

---

# 116. Build 性能目标

P1 主要任务为离线编译，因此性能不是首要指标。

但仍建议记录：

```text
wall time
peak RAM
output size
```

第一版目标原则：

> 完整地图编译应保持在普通用户可接受的分钟级范围，而不是几十分钟或小时级。

具体硬指标应在第一次 Europe build 后基于实测制定。

---

# 117. Runtime Dataset 性能优先级

虽然 compiler 可以较慢：

```text
routing.graph
```

必须为 P2 高频读取优化。

因此：

> 编译慢一点可以接受；

> Navigation Core 每次启动重新处理 JSON 不可接受。

这是选择紧凑 binary graph 的原因。

---

# 118. P1 风险 R1 — Prefab Navigation Semantics

等级：

> Critical。

原因：

这是替代 P0 全连接近似的核心。

失败意味着：

> 无法形成可信 Routing Graph。

缓解：

- semantic corpus；
-成熟项目行为对照；
- graph-debugger；
-逐 prefab family 扩展。

---

# 119. P1 风险 R2 — Road Direction Semantics

等级：

> Critical。

失败意味着：

-逆行；
-非法路线；
-时间优先等 P2 算法全部失去基础。

缓解：

- road look；
- node direction；
- telemetry driving traces；
- known road fixtures。

---

# 120. P1 风险 R3 — Full Europe Format Diversity

等级：

> High。

Berlin 能解析并不意味着：

-北欧；
-巴尔干；
-英国；
-收费站；
-边检；
-旧地图区域；

使用完全相同结构。

缓解：

> Berlin → Germany → Europe 分阶段扩展。

---

# 121. P1 风险 R4 — Definition Override

等级：

> High。

错误 archive priority 可能造成：

> parser 本身无错误，但使用了错误 definition。

因此 Resource Resolver 必须先于全欧洲 graph 完成。

---

# 122. P1 风险 R5 — Sign/Speed Semantics

等级：

> Medium/High。

静态数据可能分散于：

- road；
- country；
- sign；
-特殊 map rule。

缓解：

> 使用 Telemetry speed limit 作为 ground truth。

---

# 123. P1 风险 R6 — TruckLib License

等级：

> Medium。

缓解：

> 默认只用于外部 differential validation，不作为 production dependency。

---

# 124. P1 风险 R7 — Dataset Schema Premature Freeze

如果太早冻结 binary：

后续可能因：

- lane；
- semaphore；
- speed segment；

需求导致大改。

因此：

```text
dataset v1 schema
```

应在 Semantic Graph 稳定后冻结。

之前只使用：

```text
internal experimental schema
```

---

# 125. P1 监控指标

每次完整 build 记录：

```text
Parser:
unknown item count
unknown definition count
failed resource count

Graph:
node count
edge count
junction count
movement count
component count
largest component ratio

Validation:
fatal
error
warning

POI:
per-type count
unmatched access

Speed:
coverage
telemetry mismatches

Build:
time
peak RAM
dataset size
```

---

# 126. P1 Gate — G1 Resource

要求：

> Base + 当前安装的全部官方 DLC 自动识别和解析。

不得要求用户手工解包。

---

# 127. P1 Gate — G2 Parser

所有 P1 所需：

- sector；
- definitions；
- prefab；
- sign；
- POI；

均能自动解析。

未知但与导航无关的数据可以 Warning。

导航关键数据未知则 Error/Fatal。

---

# 128. P1 Gate — G3 Road Semantics

正式 graph 不再依赖：

```text
所有 Road 双向
```

近似。

道路方向必须由地图语义产生。

---

# 129. P1 Gate — G4 Junction Semantics

正式 graph 不再依赖：

```text
Prefab connector 全连接
```

近似。

所有 connector movement 必须来源于真实 navigation data。

---

# 130. P1 Gate — G5 Graph Validation

至少：

```text
Fatal = 0
```

已知 semantic corpus：

```text
Error = 0
```

不能简单要求整个 Europe：

```text
Warning = 0
```

因为部分 Warning 可能是合法异常。

---

# 131. P1 Gate — G6 POI

目标 POI：

```text
City
Company
Garage
Repair
Fuel
Rest
Ferry
Train
Toll
Border
```

均能生成。

需要导航的 POI 必须具有：

```text
routing access
```

---

# 132. P1 Gate — G7 Semaphore

至少对正式支持的典型路口：

```text
JunctionMovement
        ↓
Semaphore Binding
```

可以稳定建立。

---

# 133. P1 Gate — G8 Speed

能够建立：

```text
map speed profile
```

并通过 telemetry 真实数据验证其语义。

若某一规则尚不支持：

必须有明确诊断，不允许静默使用错误限速。

---

# 134. P1 Gate — G9 Dataset

以下产物一键生成：

```text
manifest.json
map.db
routing.graph
junction.graph
search.db
map.pmtiles
diagnostics.json
```

---

# 135. P1 Gate — G10 Cross-Language

独立 Rust smoke reader 可以读取：

```text
routing.graph
junction.graph
```

核心 header 和基本数据。

---

# 136. P1 Gate — G11 Determinism

相同输入：

```text
semantic hash identical
routing.graph identical
junction.graph identical
```

---

# 137. P1 Gate — G12 Europe

用户当前所有官方地图：

> 完整成功编译。

不允许：

```text
Berlin only
```

或：

```text
Germany only
```

作为 P1 结束条件。

---

# 138. P1 Gate — G13 Regression

以下全部通过：

```text
parser corpus
semantic corpus
route corpus
random OD
dataset reader
deterministic build
```

---

# 139. P1 最终 Exit Criteria

P1 正式关门的核心定义是：

\[
\boxed{
\text{能够从官方 ETS2 地图自动生成可信 Navigation Dataset}
}
\]

而不是：

\[
\boxed{
\text{能够解析地图文件}
}
\]

两者有本质区别。

---

# 140. P1 结束后的 P2 输入

P2 Navigation Core 只接受：

```text
Navigation Dataset v1
```

不读取：

```text
ETS2 archives
```

P2 应能够：

```text
load routing.graph
load map.db
load junction.graph
```

然后开始：

- Map Matching；
- A*；
- profile；
- alternative routes；
- rerouting；
- maneuver。

---

# 141. P1 与红绿灯模块的边界

P0 已经证明红绿灯 countdown 在当前方案下达到目标精度。

P1 不继续扩大 runtime reverse-engineering 工作。

P1 仅完成：

```text
map semantics
+
signal static semantics
```

即：

```text
前方哪个 movement
应该受到哪个 signal 控制。
```

P2/P3 再结合：

```text
semaphore-bridge
```

得到：

```text
当前颜色
剩余时间
```

---

# 142. P1 与车道导航的边界

P1 要保存：

- navigation lane；
- movement；
- connector。

但不要求：

> 生成“请走右侧第二车道”。

这是 V2 功能。

这样 P1 建立的数据不会在未来车道级导航时需要彻底重构。

---

# 143. P1 与 Mod 的边界

P1 Gate 只针对：

```text
ETS2 base
+
official DLC
```

未知第三方 Mod：

> 不作为 P1 测试目标。

如果资源格式恰好兼容，未来可以实验运行。

但 P1 不添加：

- ProMods offset；
-特定 prefab workaround；
-第三方 map patch。

---

# 144. P1 开发决策记录

建议新增：

```text
docs/decisions/
```

至少写 ADR：

```text
ADR-001 Resource overlay
ADR-002 Semantic model boundary
ADR-003 Dual graph design
ADR-004 Dataset format
ADR-005 TruckLib oracle-only
ADR-006 Stable ID strategy
ADR-007 POI access model
```

避免未来忘记为什么这样设计。

---

# 145. 文档同步要求

每个工作包完成后更新：

```text
PLAN.md
```

并在：

```text
docs/format-notes/
```

补充新的格式发现。

例如：

```text
prefab-descriptor.md
road-look.md
sign.md
speed-rule.md
poi.md
dataset-format.md
```

---

# 146. 当前第一步

从当前仓库状态开始，实际执行顺序应为：

```text
Step 1
P1 baseline / README / PLAN
        ↓
Step 2
GraphValidator 正式化
        ↓
Step 3
map-inspector + graph-debugger
        ↓
Step 4
Resource Resolver
        ↓
Step 5
RoadLook / Definition Model
        ↓
Step 6
Prefab Descriptor
        ↓
Step 7
JunctionMovement
        ↓
Step 8
Semantic Graph
        ↓
Step 9
删除 P0 双向/全连接近似
        ↓
Step 10
Berlin Semantic Gate
```

在 Step 10 通过之前：

> 不开始全欧洲正式编译。

---

# 147. P1 核心成功判据

整个 P1 最关键的不是代码行数，也不是解析了多少文件。

只有两个问题真正决定 P1 是否成功。

第一：

\[
\boxed{
\text{道路方向是否恢复正确}
}
\]

第二：

\[
\boxed{
\text{路口合法 movement 是否恢复正确}
}
\]

如果这两个问题解决：

> 全欧洲编译主要变成覆盖率、异常处理和测试规模问题。

如果没有解决：

> 即使能够解析全部 ETS2 官方地图，也仍然没有得到可以安全用于自主导航的路线图。

---

# 148. P1 阶段最终定义

P0 证明：

> ETS2Nav 可以读取 ETS2 地图并自动建立大规模连通图。

P1 要证明：

> ETS2Nav 可以无需人工地图修复，从 ETS2 官方地图自动建立具有真实驾驶语义的导航数据集。

因此 P1 最终完成状态应当是：

```text
Official ETS2 Installation
           │
           ▼
     Map Compiler
           │
           ▼
Verified Navigation Dataset
           │
           ▼
     Ready for P2
```

只有达到这一状态，才进入：

> P2 Navigation Core。