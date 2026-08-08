# Euro Truck Simulator 2 外部智能导航系统
## 完整需求规格与技术路线 v0.2

状态：审核修订版  
目标平台：Euro Truck Simulator 2 单人模式 + 官方地图 DLC  
核心原则：完全自主地图解析、完全自主路径规划、PC 本地导航核心、多终端显示

---

# 1. 项目目标

本项目开发一套独立于 ETS2 原生 Route Advisor 的外部智能导航系统。

系统不读取游戏已经计算出的路线，而是从用户本机 ETS2 游戏资源中解析道路、路口、信号灯、道路规则、公司、服务设施等信息，自行建立导航数据库和道路图，并完成：

- 路线搜索；
- 2～3 条不同策略候选路线；
- 实时地图匹配；
- 转向导航；
- 路口放大图；
- 红绿灯状态与倒计时；
- 红灯减速提示；
- 即将绿灯提示；
- GLOSA 绿灯建议速度；
- 当前限速、前方限速；
- 超速与测速提示；
- 偏航自动重规划；
- POI 搜索；
- 加油站和休息站规划；
- 中文语音导航；
- PC 或移动设备导航界面。

视觉和交互目标参考现代高德地图导航模式，但图标、地图样式、动画素材、声音以及程序实现均自行制作，不直接复制高德私有素材。

---

# 2. 正式支持范围

V1 正式支持：

- ETS2；
- 官方基础地图；
- 用户已经安装的官方地图 DLC；
- Windows；
- 单人模式；
- PC 本地运行导航核心；
- PC UI；
- Android/iOS/平板/浏览器局域网客户端。

不支持：

- TruckersMP；
- ATS；
- 动态联网交通拥堵；
- 开发者为 ProMods 等第三方地图逐个制作适配层。

第三方地图 Mod 可以尝试自动解析，但属于实验兼容，不纳入正式验收范围。

这一调整来自现有生态的现实情况：TruckSim Maps 当前明确支持官方 DLC、明确不支持第三方地图 Mod；TruckNav 同样明确不支持地图 Mod。

---

# 3. 总体架构

采用：

```text
                     ETS2
                      │
              SCS Telemetry SDK
                      │
                      ▼
             ┌──────────────────┐
             │ Telemetry Bridge │
             │    C++ DLL       │
             └────────┬─────────┘
                      │ Shared Memory
                      ▼
        ┌───────────────────────────────┐
        │       Navigation Core         │
        │                               │
        │ Map Matching                  │
        │ Routing                       │
        │ Maneuver Generation           │
        │ Traffic Light Engine          │
        │ Warning Engine                │
        │ Search / POI                  │
        │ TTS Event Generator           │
        └──────┬─────────────────┬──────┘
               │                 │
               │                 │ HTTP / WebSocket
               ▼                 ▼
          Windows UI        Mobile / Browser
                               Renderer
```

另设完全独立的：

```text
ETS2 Files
   │
   ▼
Map Compiler
   │
   ├── map.db
   ├── routing.graph
   ├── junction.graph
   ├── search.db
   └── map.pmtiles
```

Map Compiler 是低频离线任务。

Navigation Core 是游戏运行期间的高频实时任务。

两者严格解耦。

---

# 4. 为什么核心放在 PC

最终建议确定采用：

> PC Navigation Core + PC/Mobile 双前端。

原因是 ETS2 Telemetry、地图文件以及导航数据库均在 PC，本地完成计算可以避免移动设备承担：

- 地图解析；
- 全欧洲路径规划；
- 地图匹配；
- 红绿灯逻辑；
- 游戏状态同步。

如果使用手机或平板显示，则 PC 甚至不需要运行 MapLibre 图形渲染，只需要进行 CPU 侧导航计算。

因此理论性能开销很小。

性能是否真正“对 ETS2 无明显影响”不通过主观判断确定，而通过最终基准测试验收。

---

# 5. 游戏数据获取

SCS 官方 Telemetry SDK 当前官方页面仍列出稳定版本 1.14，提供玩家车辆数据供第三方应用使用。

Telemetry Bridge 需要至少提供：

```text
position
orientation
speed
pause state
simulation timestamp
current speed limit
fuel
fuel range
job state
destination city
destination company
rest/fatigue related state
```

最终字段名称必须以实际 SDK 头文件为准，而不能依赖第三方 wrapper 的字段命名。

---

# 6. Telemetry Bridge

建议自行开发轻量 C++ DLL。

DLL 内不执行：

- 路线规划；
- 地图解析；
- UI；
- 网络服务器；
- 复杂算法。

它只负责：

```text
ETS2 SDK callback
       ↓
规范化数据
       ↓
Shared Memory
```

原因是插件运行于 ETS2 进程中。

应当尽量缩小：

- CPU 时间；
- 内存分配；
- 锁竞争；
- 异常风险。

Navigation Core 崩溃不能导致 ETS2 崩溃。

---

# 7. PC 进程间通信

Telemetry Bridge → Navigation Core 推荐：

> Shared Memory + sequence counter。

而不是：

- 高频 JSON；
- TCP localhost；
- 文件写入。

现有 SCS 插件已经长期采用 Memory Mapped File 传输 telemetry，因此技术路线成熟。

---

# 8. 地图初始化

用户第一次安装系统后：

```text
检测 ETS2
   ↓
检测官方 DLC
   ↓
扫描游戏 archives
   ↓
解析 map / def / prefab 等资源
   ↓
构建静态地图模型
   ↓
构建 Junction Graph
   ↓
构建 Routing Graph
   ↓
构建 POI/Search Index
   ↓
生成 Vector Tiles
   ↓
运行完整 Graph Validation
   ↓
生成 Navigation Database
```

这一过程允许耗时数分钟。

TruckSim Maps 已经证明可以从 ETS2 文件自动生成 JSON、GeoJSON 和 PMTiles，而且完整 DLC 解析本身就是分钟级离线任务。

因此地图编译不得放入游戏实时循环中。

---

# 9. 地图更新检测

生成资源 fingerprint：

```text
game version
base archive metadata
DLC list
DLC archive metadata
map sector hashes
definition hashes
enabled mods
mod order
parser schema version
```

启动 Navigation Core 时比较 fingerprint。

如果完全一致：

> 直接使用缓存。

如果只是地图内容改变：

> 自动重新编译。

如果检测到无法识别的地图文件格式：

> 禁止继续使用旧数据库冒充兼容；
> 提示当前游戏版本需要更新导航解析器。

因此应把此前的：

> “只有官方 SDK/API 更新才需要更新软件”

修正为：

> 普通地图内容更新应当自动兼容；  
> Telemetry SDK、地图二进制格式、资源组织结构或字段语义发生不兼容变化时，需要更新软件。

---

# 10. 第三方 Mod

采用三档状态：

```text
VERIFIED
EXPERIMENTAL
UNSUPPORTED
```

官方地图：

> VERIFIED。

未知 Mod：

> EXPERIMENTAL。

如果解析器能够处理其：

- sector；
- prefab；
- road；
- semaphore；
- POI；
- resource override；

且完整 graph validation 通过，则允许用户运行。

但 UI 应明确显示：

> 第三方地图，未验证。

不能声称：

> “格式能打开 = 导航一定正确”。

如果：

- 坐标变换异常；
- 未知 prefab；
- graph validation 失败；
- 无法解析 override；

则：

> UNSUPPORTED。

软件本身不针对该 Mod 开发适配。

---

# 11. Map Compiler 技术原则

现有项目证明地图解析是可行的，但也证明它不是成熟标准 API。

TruckSim Maps 的 parser 能够处理官方 DLC，但其 README 明确指出生成的道路/prefab GeoJSON “far from perfect”，很多交叉路口形状仍不正确。

TruckNav 则明确说明，为获得可用 routing graph，作者进行了大量 QGIS 和脚本修复，仍存在断路和非法掉头。

因此本项目必须区分：

```text
地图几何解析
≠
导航拓扑恢复
```

导航拓扑恢复是单独的核心工程。

---

# 12. 地图编译器语言

不再要求整个项目全部使用 Rust。

推荐：

### Telemetry

C++。

### Navigation Runtime

Rust。

### Map Compiler

C# 或 TypeScript。

优先级可以定为：

> C# > TypeScript > Rust。

这里语言本身不是核心问题，关键是：

- 便于二进制解析；
- 快速验证格式；
- 编译阶段不要求极低延迟。

现有最成熟的地图解析生态主要集中在 C# TruckLib 和 TypeScript TruckSim Maps，而不是 Rust。

但需要注意：TruckLib 是 GPL-2.0，TruckSim Maps 是 GPL-3.0。

因此，在项目许可证尚未确定前，建议默认：

> 不直接复制或链接 GPL 实现。

可将这些项目用于：

- 技术研究；
- 输出结果对照；
- 行为验证；

生产 parser 采用独立实现。

如果未来项目本身决定采用 GPL，则可以重新评估直接复用。

---

# 13. 内部地图数据模型

不采用单一 graph。

至少建立三级模型。

## Level 1：Map Geometry

保存：

- Road；
- Prefab；
- Node；
- Sign；
- City；
- Company；
- Services；
- Ferry；
- Train；
- Semaphore。

负责：

> “地图上有什么”。

## Level 2：Routing Graph

负责：

> “车辆从哪里能够合法到哪里”。

## Level 3：Junction/Lane Graph

负责：

> “在某个路口内应该经过哪一条通行路径”。

V1 即建立 Level 3 数据，但暂时不一定对用户提供车道级导航。

这样 V2 无需重新设计数据层。

---

# 14. Routing Graph

每条有向 edge 至少保存：

```text
edge_id
from_node
to_node

geometry
length

road_class
road_number
road_name

speed_limit

country
city

direction

junction metadata

traffic_light_group
speed_camera

toll
ferry
train

service access

turn restrictions
```

采用紧凑数组/CSR 类结构。

不建议使用大量面向对象节点实例存储整个欧洲路网。

---

# 15. Junction Graph

Prefab 内部导航不能只根据道路中心线猜测。

SCS prefab 数据明确区分 Map Point 与 Navigation Point，而且 semaphore profile 本身存在 interval 和 cycle 配置。

因此应：

```text
incoming road
     ↓
prefab connector
     ↓
navigation path
     ↓
traffic semaphore
     ↓
outgoing road
```

直接建立路口内部通行关系。

这也是未来实现：

- 左转/右转；
- 环岛；
- 复杂立交；
- 信号灯关联；
- 车道导航；

的基础。

---

# 16. 自动路由图生成风险

这一项列为：

> P0-A：最高工程风险。

目标仍然是：

> 用户无需人工修地图。

但这不等于：

> 系统永远不会遇到自动无法修复的问题。

正确设计应为：

```text
Automatic Build
      ↓
Semantic Validation
      ↓
Topology Validation
      ↓
Regression Validation
      ↓
PASS → 正常使用

FAIL
 ↓
隔离异常区域
 ↓
生成诊断报告
```

而不是：

> validation 检测出错误后仍继续导航。

---

# 17. Graph Validation

至少实现：

### 结构检测

- isolated components；
- dangling road；
- invalid references；
- disconnected prefab；
- duplicate node；
- self loop。

### 方向检测

- illegal reverse edge；
- one-way contradiction；
- impossible turn；
- illegal U-turn。

### Junction 检测

- missing entrance；
- missing exit；
- impossible maneuver；
- roundabout connectivity；
- company entrance connectivity。

### 几何检测

- route geometry discontinuity；
- extreme heading jump；
- edge overlap anomaly。

### 语义检测

- motorway 突然连接不合理乡村小路；
- route 穿越 prefab 非 Navigation Path 区域；
- 信号灯与入口方向不一致。

需要特别认识到：

> Validation 可以证明“发现了很多错误”，但不能证明“没有语义错误”。

因此还必须建设 Regression Dataset。

---

# 18. 路由图回归测试集

建立一组已知路线：

```text
origin
destination
expected mandatory roads
forbidden maneuvers
expected ferry/train use
roundabout exit
company entrance
```

覆盖：

- 英国；
- 法国；
- 德国；
- 北欧；
- 巴尔干；
- 意大利；
- 西班牙；
- 东欧；
- 官方最新 DLC 区域。

每次地图编译器修改后自动重跑。

此外随机生成数千个 OD pair，检查：

- 可达性；
- geometry 连续；
- 不合理掉头；
- graph jump。

---

# 19. 用户运行期间的众包式自验证

可在本机后台记录：

```text
actual vehicle trajectory
↕
map-matched route edge
```

用于验证：

- 实际车辆确实能沿 edge 通行；
- speed limit parser 是否正确；
- junction connection 是否正确。

这些数据仅保存在本地。

若发现：

```text
Telemetry speed limit
!=
Map Compiler speed limit
```

则写入 diagnostics。

这样软件使用越多，自身的验证数据越完整。

---

# 20. 路线搜索总体模型

不采用简单：

> 最短距离。

而建立统一 edge cost：

\[
C_e =
w_tT_e+
w_dD_e+
w_sS_e+
w_jJ_e+
w_rR_e+
w_fF_e .
\]

其中：

- \(T_e\)：预计自由流通行时间；
- \(D_e\)：距离；
- \(S_e\)：信号灯预计延误；
- \(J_e\)：路口和复杂转向成本；
- \(R_e\)：道路等级/低速道路成本；
- \(F_e\)：渡轮、收费等附加成本。

---

# 21. 红绿灯如何进入路线规划

这里必须区分：

> 路线规划中的红绿灯统计延误

和：

> 驾驶过程中前方红绿灯实时剩余秒数。

两者不是同一个问题。

即使无法实时获得远方红绿灯当前相位，也仍然可以根据 signal profile 计算预期等待时间。

对于某一个 movement，假设整个周期为 \(C\)，不可通行阶段总长度为 \(R\)，若随机到达近似均匀，则最简单情形的平均等待可近似：

\[
E(W)=\frac{R^2}{2C}.
\]

实际实现不需要拘泥于此解析式。

可以将完整 signal state machine 离散积分：

\[
E(W)
=
\frac{1}{C}
\int_0^C W(\phi)\,d\phi .
\]

每一种 semaphore movement 在地图初始化时预计算一次。

运行时 route search 只读取一个数字。

因此不会产生明显性能开销。

---

# 22. 默认路线策略

V1 默认提供三种：

### 推荐

综合考虑：

- 行驶时间；
- 信号灯；
- 复杂路口；
- 道路等级；
- 距离。

### 时间优先

主要优化：

\[
ETA.
\]

允许一定绕行以提高平均速度或减少信号灯。

### 距离优先

主要优化：

\[
Distance.
\]

但仍必须遵守：

- 单向；
- 禁止转弯；
- 不可通行道路。

后续可以增加：

- 少红绿灯；
- 高速优先；
- 避免收费。

---

# 23. 多路线生成

不能简单输出三条高度重叠路线。

流程：

```text
Profile 1
Profile 2
Profile 3
   ↓
独立 Route Search
   ↓
Overlap Ratio
   ↓
去除高度重叠
   ↓
必要时增加 overlap penalty
   ↓
重新搜索
```

如果只有一条实际上合理的路线：

> 只显示一条。

不得为了凑够三条输出明显不合理绕行。

---

# 24. Route Search 算法

第一阶段：

> A*。

Heuristic：

\[
h(n)=\frac{d(n,g)}{v_{\max}}.
\]

确保 admissible。

性能不足后升级：

- ALT；
- Landmarks；
- Customizable Contraction Hierarchies。

不建议第一阶段直接使用复杂 CH。

因为当前最大的未知量是：

> graph 是否正确，

而不是：

> A* 是否够快。

---

# 25. 车辆 Map Matching

实时车辆位置不能简单匹配最近 edge。

候选道路评分：

\[
P(e_t|x_t)
\propto
P(d)
P(\theta)
P(e_t|e_{t-1})
P(v).
\]

考虑：

- 横向距离；
- 航向差；
- 前一 edge；
- 拓扑可达性；
- 车辆速度。

重点处理：

- 高速上下层；
- 平行辅路；
- 对向车道；
- 环岛；
- 服务区；
- 公司停车场。

V1 推荐：

> sliding-window HMM / Viterbi

或者具有拓扑约束的递归 candidate tracker。

采样频率：

> 10～20 Hz 足够。

---

# 26. 偏航判断

不能因为瞬间 GPS-like projection 偏差就重算。

满足以下条件之一再确认：

- 连续多帧最优 edge 不在 route corridor；
- 实际已经沿新 edge 前进一定距离；
- route probability 低于阈值持续一定时间。

随后：

```text
current matched edge
      ↓
new routing origin
      ↓
preserve destination/profile
      ↓
route search
```

目标：

> 偏航确认后 1 s 左右完成重规划。

---

# 27. 红绿灯系统重新定义

红绿灯是：

> P0-B：最高单功能可行性风险。

官方/社区资料能够确认：

```text
interval = Green / Orange / Red / Orange
cycle = phase shift
```

等静态 profile 信息。

但 SCS Telemetry SDK 没有公开“当前灯状态 + 剩余秒数”接口。

所以真正的问题不是：

> “周期数据能不能读？”

而是：

> “游戏运行时这一盏灯当前处于周期中的哪个位置？”

---

# 28. 不再预设信号灯时钟

此前评估文档提出：

> interval 是模拟秒，需要按城市 1:3、高速 1:19 换算。

这一结论目前证据不足。

可以确认的是：

> `warp` 会改变包括信号灯在内的整个 simulation 动画速度。

但：

> ETS2 地图行程时间压缩倍率是否同样驱动 semaphore timer

尚未获得足够证据。

因此 v0.2 禁止在设计阶段预设任何时钟模型。

---

# 29. Traffic Light Clock-Domain 实验

同时记录：

```text
Windows monotonic clock
render_time
simulation_time
paused_simulation_time
available game-clock values
warp setting
signal phase transition
```

SDK 示例明确表明 simulation timer 在 load 等情况下可能 restart，因此不能把它未经验证地视为永远连续的绝对时钟。

然后比较：

\[
\Delta t_\mathrm{signal}
\]

分别与各种 clock 的：

\[
\Delta t
\]

是否成稳定比例。

由实验判断真正 clock domain。

---

# 30. 信号灯相位锚定实验

现有社区观察提出一种非常重要的可能：

> 信号灯可能在进入玩家加载范围后从某种默认状态启动，而不是从全世界统一绝对时钟读取相位。

论坛中已有玩家明确报告“灯在一定距离生成并从相似状态运行”的现象，但这属于社区观察而非官方规范。

所以必须检验两个模型。

### H1：Global Clock Model

\[
\phi=f(t_\mathrm{global})
\]

如果成立：

> 纯计算倒计时。

### H2：Local Spawn Clock Model

\[
\phi=f(t-t_\mathrm{load})
\]

如果成立：

> 必须先取得一次运行时 phase anchor。

---

# 31. Phase Anchor 获取策略

优先顺序：

### A. 静态计算

如果 H1 成立，直接计算。

### B. 视觉锚定 + profile 外推

如果 H2 成立：

```text
Screen Capture
     ↓
Traffic Light Detection
     ↓
识别一次 phase transition
     ↓
anchor timestamp
     ↓
利用 profile 外推后续周期
```

这种方式不要求每一帧都重新识别灯。

只要确认：

> 某盏灯在 \(t_0\) 时刻切换到 GREEN，

后续：

\[
\phi(t)=
(t-t_0+\phi_0)\bmod C.
\]

### C. 运行时内部状态读取

不作为 V1 基线。

原因：

- 非官方；
- 地址和结构随版本变化；
- 维护成本高。

虽然本项目明确不支持 TruckersMP，技术上没有多人反作弊兼容要求，但仍不建议把内存逆向作为第一方案。

---

# 32. 视觉识别设计

如果需要视觉 anchor：

PC 通过：

- Windows Graphics Capture；
- 或 DXGI Desktop Duplication

获取 ETS2 窗口。

Detector 只识别：

```text
RED
RED_AMBER
GREEN
AMBER
UNKNOWN
```

通过 temporal tracking 关联同一灯组。

地图中的：

- 前方路口；
- 预期 semaphore group；
- 车辆路线；

可以大幅降低识别候选范围。

不需要对整个画面所有灯都进行高成本识别。

---

# 33. 红绿灯倒计时精度标准

用户要求：

\[
|t_\mathrm{display}-t_\mathrm{actual}|
\leq1.0\,\mathrm{s}.
\]

定义为硬条件。

支持倒计时必须同时满足：

```text
signal profile known
phase anchor reliable
clock model known
semaphore association reliable
confidence >= threshold
```

否则：

```text
COUNTDOWN_VERIFIED
STATE_ONLY
UNAVAILABLE
```

只能在 `COUNTDOWN_VERIFIED` 状态显示具体数字。

这一原则保留此前方案的正确部分，也与评估报告建议一致。

---

# 34. 红绿灯特殊模式

`semaphore_profile` 还可能存在：

- sleep time；
- 非标准 profile；
- ramp meter；
- blockable light；
- barrier。

例如社区 profile 实例确实使用 `sleep_time_start/end`。

因此不能简单写死：

> 23:30～03:00 所有信号灯都闪烁。

正确规则是：

> 解析每个 profile 自身状态。

对于不能建立标准周期的模式：

> 禁止显示倒计时。

---

# 35. 前方红绿灯匹配

不能采用：

> 最近 traffic light。

而应：

```text
current edge
  ↓
planned outgoing edge
  ↓
junction movement
  ↓
navigation path
  ↓
associated semaphore
```

因此可以区分：

- 本车直行；
- 左转；
- 右转；
- 横向车流；
- 对向车辆。

只有控制当前 planned movement 的灯才进入 UI。

---

# 36. 红灯减速提示

基本停车距离：

\[
d_\mathrm{stop}
=
vt_r+
\frac{v^2}{2a}
+d_m.
\]

参数不追求卡车动力学仿真级精度。

因为实际制动受：

- 挂车；
- 货物；
- 车辆设置；
- 游戏物理参数；

影响。

采用保守驾驶辅助模型。

例如：

```text
d_signal < d_warning(v)
AND
signal != GREEN
```

触发：

> 前方红灯，请减速。

---

# 37. 即将绿灯

条件：

```text
state == RED
countdown_verified
vehicle speed < threshold
remaining < threshold
```

例如：

> 即将绿灯。

不应在车辆高速接近路口时播报“即将绿灯”，以免诱导驾驶者加速。

---

# 38. GLOSA

设绿色通行时间窗口：

\[
[t_1,t_2]
\]

距停止线：

\[
d.
\]

理论速度范围：

\[
\frac d{t_2}
\leq v
\leq
\frac d{t_1}.
\]

再与：

- 当前限速；
- 前方限速；
- 合理加速度；
- 合理减速度；

求交集。

最终 UI 不显示过高精度。

例如数学范围：

\[
53.6\sim64.2\,\mathrm{km/h}
\]

可显示：

> 建议 55–60 km/h

或者：

> 50–60 km/h。

需要强调：

> 输出范围并不会显著降低 CPU 开销。

计算本身极轻。

这样设计主要是为了避免制造虚假精度，让驾驶者有自然速度区间。

更新频率：

> 2～5 Hz。

不需要 20～60 Hz。

---

# 39. 当前限速

数据源双轨：

### Telemetry

作为车辆当前所在位置的实时参考。

### Map Database

用于：

- 前方限速；
- 路线 ETA；
- 导航提示。

运行时比较：

```text
telemetry limit
vs
parsed map limit
```

若不一致：

> diagnostic event。

这将成为 speed-limit parser 最有价值的 ground truth。

---

# 40. 前方限速

Map Compiler 应建立沿 edge 的 speed-limit interval：

```text
edge
 ├─ 0–250 m: 80
 ├─ 250–700 m: 50
 └─ 700 m+: 70
```

因此可以显示：

> 前方 300 m 限速 50。

而不是仅知道当前限速。

---

# 41. 超速提醒

配置：

- 提醒阈值；
- 是否播报；
- 声音；
- 重复间隔。

建议默认：

```text
≤50 km/h：
+3 km/h

>50 km/h：
+5 km/h
```

但全部可自定义。

---

# 42. 测速摄像头

功能要求保留。

但其在地图文件中的编码覆盖率尚未完整验证，因此定义为：

> V1 P1 数据验证项目。

如果可以稳定提取：

- position；
- controlled direction；
- speed limit；

则实现：

> 前方 500 m 测速。

如果数据无法可靠识别，则不得通过模型名称猜测并造成大量误报。

---

# 43. 转向 Maneuver Generator

Route 转换为 maneuver list。

支持：

```text
STRAIGHT
TURN_LEFT
TURN_RIGHT
SLIGHT_LEFT
SLIGHT_RIGHT
KEEP_LEFT
KEEP_RIGHT
U_TURN
ROUNDABOUT
MOTORWAY_ENTER
MOTORWAY_EXIT
FERRY
TRAIN
TOLL
BORDER
```

不能只按两个 edge 的夹角判断。

还需要：

- junction type；
- road class；
- continuation road；
- roundabout topology。

---

# 44. 环岛

识别闭合环岛结构。

从 entry connector 开始，对合法 outgoing branch 计数。

输出：

> 从环岛第三出口驶出。

不得生成：

```text
右转
右转
右转
```

这种错误导航。

---

# 45. 2D 路口示意图

不制作每个路口的固定图片。

实时从 Junction Graph 生成：

```text
junction graph
      ↓
附近道路 geometry
      ↓
simplification
      ↓
planned maneuver highlighting
      ↓
SVG / Canvas
```

临近路口：

> 自动切换 Junction View。

驶离：

> 恢复正常导航地图。

---

# 46. 车道级导航

纳入 V2。

V1 地图数据库仍保留 lane/navigation path 信息。

V2 再解决：

```text
road lane
   ↕
prefab navigation lane
   ↕
outgoing road lane
```

最大难点不在 UI 箭头，而是：

> 如何可靠跨 prefab 边界保持 lane continuity。

因此不建议提前加入 V1 验收。

---

# 47. 高速出口编号

列为：

> Conditional V1。

如果 sign/parser 可以可靠得到：

- road number；
- exit number；
- direction text；

显示：

> 从 12 号出口驶出。

否则：

> 前方出口驶出。

不能自己生成编号。

---

# 48. 语音导航

核心层只生成语义：

```text
{
  type: "TURN_RIGHT",
  distance: 500,
  exit: null
}
```

语音客户端转换：

> 前方五百米右转。

这样能够支持：

- 中文；
- 英文；
- 不同播报频率；
- 不同话术。

V1 中文必须完成。

英文属于次优先级。

---

# 49. 播报频率

不将所有路况固定为：

```text
2 km
1 km
500 m
250 m
100 m
```

而采用：

\[
D=f(v,\text{road class},\text{maneuver complexity}).
\]

用户再选择：

### 低频

只选择关键 trigger。

### 标准

提供完整预告。

### 高频

增加更多提前节点。

仍允许高级设置直接配置：

```text
2000
1000
500
250
100
```

等距离。

---

# 50. POI 搜索

V1 支持：

- 城市；
- 公司；
- 维修站；
- 车库；
- 加油站；
- 服务区；
- 休息站；
- 渡轮；
- 火车运输；
- 收费站；
- 国境/检查站。

建立本地搜索索引。

支持：

- 名称；
- 城市；
- 类型；

搜索。

完全离线。

---

# 51. 当前货运任务目的地

读取 job destination：

```text
destination city
destination company
internal ID
```

通过本地 POI DB 映射到：

> company access node。

若 company 存在多个入口：

> 选择 routing graph 中合法且距卸货区域合理的入口。

不能简单导航到公司几何中心。

---

# 52. 加油站规划

支持：

> 沿途加油站。

以及：

> 最近加油站。

如果 telemetry 可以取得稳定 fuel range：

\[
R_\mathrm{fuel}.
\]

当：

\[
D_\mathrm{destination}
>
R_\mathrm{fuel}-M
\]

提示：

> 建议沿途加油。

V1 不做油价优化。

---

# 53. 休息站规划

支持：

- 最近休息站；
- 沿途休息站；
- 将休息站作为临时 waypoint。

如果疲劳 telemetry 可用：

> 可以主动建议休息。

但不加入：

- 货运利润优化；
- 交货 deadline 风险模型。

这符合当前范围。

---

# 54. 静态道路事件与动态事件

必须明确区分。

### 静态

地图固定：

- 收费站；
- 渡轮；
- 国境；
- 检查站；
- 固定施工；
- 固定特殊限速。

可以解析。

### 动态

游戏随机产生：

- 事故；
- 临时施工；
- 随机事件；
- 临时封闭。

除非未来找到可靠运行时接口，否则：

> V1 不保证识别。

不能在 UI 中虚构“实时路况”。

---

# 55. 当前道路名称

优先：

```text
road name
↓
road number
↓
city / road class
↓
无
```

ETS2 并非所有路段都有现实世界道路名。

因此：

> 没有数据时不显示。

---

# 56. UI 总体布局

参考现代高德导航的信息结构：

```text
┌──────────────────────────┐
│        下一转向卡片       │
│   ↱ 500 m 进入高速        │
├──────────────────────────┤
│                          │
│          地图            │
│                          │
│     ━━━━━ route          │
│           ▲truck         │
│                          │
│ [80]   83 km/h           │
│                          │
│   红灯  12 s             │
│ 建议 50–60 km/h          │
├──────────────────────────┤
│ 剩余距离 / 路线 / 设置    │
└──────────────────────────┘
```

---

# 57. 自动缩放

缩放函数：

\[
Z=
f(
v,
D_\mathrm{maneuver},
junction complexity
).
\]

高速：

> 显示较远范围。

城市：

> 显示附近道路。

复杂路口：

> 自动进入 Junction View。

用户手动拖动地图后：

> 临时暂停 follow mode。

随后自动恢复。

---

# 58. PC UI 和移动端 UI

推荐共享：

```text
TypeScript
React 或 Vue
MapLibre GL
```

PC：

> Tauri。

移动端：

> Capacitor / PWA。

不建议 V1 分别开发：

- Windows 原生 UI；
- Android 原生 UI；
- iOS 原生 UI；

三套完全不同的渲染代码。

TruckNav 已经证明 Desktop、Android、Browser 共享导航产品结构是可行路线。

---

# 59. 地图渲染

静态地图生成：

> PMTiles / MVT。

MapLibre 负责绘制。

路线算法始终使用：

> ETS2 native coordinate。

渲染阶段才转换为 MapLibre 可使用的坐标。

不应让 WGS84 转换误差污染路径规划。

---

# 60. PC → 移动端 API

HTTP：

```text
GET /map/metadata
GET /search
POST /route
POST /route/select
GET /settings
```

WebSocket：

```text
vehicle
route_progress
maneuver
speed_limit
traffic_light
warning
map_state
```

Telemetry 内部可以 20～50 Hz。

网络端只需：

> 10～20 Hz。

移动端通过插值实现 60 FPS truck icon animation。

---

# 61. 局域网连接

PC 启动后：

```text
Nav Server
192.168.1.20:xxxx
```

生成二维码：

```text
IP
port
session token
```

移动设备扫码连接。

服务默认：

- localhost；
- RFC1918/private LAN。

不默认监听公网接口。

---

# 62. 性能目标

PC Core 游戏运行状态：

### CPU

目标：

> 现代桌面 CPU 总 CPU 占用约 1～3% 以内。

### RAM

目标：

> < 500 MB。

### Disk

steady state：

> 不持续高频写盘。

### Map Matching

> 10～20 Hz。

### Routing

典型：

> < 1 s。

极端：

> < 2 s。

### Rerouting

确认偏航后：

> ≈1 s。

### LAN

Core → Mobile：

> < 200 ms。

---

# 63. 对游戏性能的正式验收

测试两组：

```text
ETS2 only
```

与：

```text
ETS2 + Nav Core
```

固定：

- 路线；
- 天气；
- 画质；
- traffic；
- camera path。

记录：

- average FPS；
- 1% low；
- CPU frametime；
- GPU frametime；
- CPU package usage；
- RAM。

目标：

PC UI 关闭、仅 Core + Mobile Renderer 时：

> 平均 FPS 差异 ≤ 1～2%；

> 1% low 回退 ≤ 2%；

若超过：

> 必须 profile 优化。

因此“PC Core 不影响游戏”最终由 benchmark 定义，而不是作为无条件假设。

---

# 64. 红绿灯 P0 验证方案

这是新版方案相对于 v0.1 最重要的修改之一。

原评估建议扩充为：

- 相位锚定；
- 时间倍率；
- load/reset；
- sleep mode。

审核后进一步修改为五项。

## TL-01：Clock Domain

同时测量不同 clock。

确认 semaphore interval 究竟使用什么时间。

## TL-02：Phase Anchor

多个不同游戏时间进入同一路口。

判断：

\[
\phi=f(global\ time)
\]

还是：

\[
\phi=f(load\ event).
\]

## TL-03：Warp

分别：

```text
warp 0.5
warp 1
warp 2
```

测信号周期。

## TL-04：Reset

测试：

- load save；
- quick travel；
- teleport；
- 远距离驶离再返回；
- pause。

## TL-05：Special Profiles

测试：

- sleep_time；
- dedicated left；
- standard junction；
- complex junction；
- blockable semaphore。

---

# 65. 红绿灯 Go/No-Go

### Case A

纯静态计算：

\[
\max |e|\le1s
\]

通过。

→ V1 全面支持。

### Case B

纯计算失败，但视觉 anchor 后：

\[
\max |e|\le1s
\]

通过。

→ V1 使用 hybrid mode。

### Case C

只能可靠判断颜色：

→ `STATE_ONLY`。

不显示秒数，不提供 GLOSA。

### Case D

颜色也不能可靠对应当前 movement：

→ `UNAVAILABLE`。

---

# 66. 路由图 P0 验证

与红绿灯并行，不应等红绿灯完成后才开始。

选取一个城市及外围高速区域，包含：

- 普通十字路口；
- 环岛；
- 高速入口；
- 高速出口；
- 公司；
- 加油站。

完成：

```text
raw sector
↓
prefab
↓
navigation paths
↓
directed graph
↓
route
```

然后测试至少：

> 100～500 个随机 OD。

重点检查：

- 断路；
- 非法掉头；
- 逆行；
- 错误路口转向；
- company entrance。

只有证明不依赖人工 QGIS 才进入全欧洲地图阶段。

---

# 67. 风险分类

不再简单排成一条排行榜。

## 最高工程风险

> Automatic Routing Graph Generation。

原因：

公开项目已经证明这部分会需要大量人工修正。

## 最高功能可行性风险

> Traffic Light ±1 s Countdown。

原因：

公开 SDK 没有运行时 signal phase 通道。

## 中等风险

- speed camera extraction；
- sign/exit number；
- speed-limit propagation；
- dynamic event recognition。

## 低风险

- Telemetry；
- POI；
- A*；
- rerouting；
- TTS；
- LAN；
- PC/mobile UI。

---

# 68. V1 正式功能

只有进入正式 Release 时才要求：

### 基础

- Telemetry；
- 自建地图；
- 官方 DLC；
- 自主路线；
- 多路线；
- Map Matching；
- 偏航重规划。

### 导航

- 转向；
- 环岛；
- Junction View；
- 当前速度；
- 当前限速；
- 前方限速；
- 剩余距离；
- 当前道路。

### 提醒

- 超速；
- 加油站；
- 休息站；
- 收费站；
- 渡轮；
- 火车；
- 国境/检查站；
- 可可靠解析的测速摄像头。

### 搜索

- 城市；
- 公司；
- 加油；
- 维修站；
- 车库；
- POI；
- 当前任务目的地。

### UI

- PC；
- LAN mobile；
- 自动缩放；
- 路口放大；
- 中文语音。

---

# 69. 红绿灯功能的 V1 定义

产品需求仍然把以下视为目标：

- countdown；
- braking warning；
- green-soon warning；
- GLOSA。

但工程发布条件改为：

> P0 达到 ±1 s 后进入正式 V1。

这里不是降低需求。

用户要求的：

\[
\pm1s
\]

保持不变。

变化的是：

> 在技术验证之前不得把尚未证实的能力当作已经可实现的事实。

---

# 70. V2

主要包括：

- lane-level guidance；
- lane arrows；
- 更复杂 motorway junction；
- incremental map compilation；
- advanced route profiles；
- multi-waypoint；
- 更完善 Mod diagnostics；
- 高质量统一中文离线 TTS；
- 更复杂 route editing。

---

# 71. 不纳入当前范围

明确暂不开发：

- TruckersMP；
- 在线交通拥堵；
- 货运收益优化；
- 交货 deadline 风险；
- AI 自动驾驶；
- 现实 GPS；
- 现实世界高德地图叠加；
- ProMods 专门适配；
- 通过网络服务器计算路线。

所有导航核心：

> 本地运行。

---

# 72. 推荐项目目录

```text
ets2-nav/
│
├─ telemetry-plugin/
│   ├─ sdk/
│   └─ shared-memory/
│
├─ map-compiler/
│   ├─ archive/
│   ├─ sector/
│   ├─ sii/
│   ├─ prefab/
│   ├─ navigation-path/
│   ├─ semaphore/
│   ├─ sign/
│   ├─ poi/
│   ├─ graph-builder/
│   ├─ graph-validator/
│   └─ tile-generator/
│
├─ map-diagnostics/
│   ├─ graph-viewer/
│   ├─ anomaly-report/
│   └─ regression-dataset/
│
├─ nav-core/
│   ├─ telemetry/
│   ├─ map-matching/
│   ├─ routing/
│   ├─ route-alternatives/
│   ├─ maneuver/
│   ├─ traffic-light/
│   ├─ glosa/
│   ├─ warnings/
│   ├─ poi/
│   └─ state/
│
├─ nav-server/
│   ├─ http/
│   ├─ websocket/
│   └─ auth/
│
├─ ui/
│   ├─ desktop/
│   └─ shared/
│
├─ mobile/
│
└─ tests/
    ├─ telemetry/
    ├─ map-parser/
    ├─ graph/
    ├─ routing/
    ├─ traffic-light/
    ├─ map-matching/
    └─ performance/
```

---

# 73. 正确开发顺序

## Phase P0

只验证两个最大风险：

```text
P0-A Routing Graph
P0-B Traffic Light
```

并行进行。

此时不要开发正式高德式 UI。

只用 Debug UI。

---

## Phase P1

建立完整：

> Map Compiler。

支持：

- 官方 base map；
- 官方 DLC；
- graph；
- POI；
- sign；
- semaphore；
- vector tile。

---

## Phase P2

建立：

> Navigation Core。

完成：

- Map Matching；
- A*；
- route profile；
- alternatives；
- rerouting；
- maneuvers。

---

## Phase P3

完成：

> Driving Assistant。

- speed limit；
- overspeed；
- camera；
- traffic light；
- GLOSA。

---

## Phase P4

正式 UI。

顺序：

```text
Browser Debug UI
↓
Desktop
↓
LAN Mobile
↓
Android/iOS packaging
```

---

## Phase P5

全欧洲官方地图测试。

建立：

> automated OD corpus。

---

## Phase P6

性能优化和正式发布。

---

# 74. P0 最小可交付物

第一阶段不追求“导航软件”。

应该只产生几个开发工具。

### `telemetry-dump`

显示：

```text
position
heading
speed
timestamps
speed limit
job
```

### `map-inspector`

显示：

```text
roads
prefabs
navigation points
semaphores
```

### `graph-debugger`

能够：

> 点击 A/B 两点并显示计算路线。

### `signal-lab`

实时显示：

```text
signal profile
expected state
detected state
timer
prediction error
```

这四个工具验证通过之后才值得投入大量 UI 工作。

---

# 75. P0 核心验收

项目进入正式开发必须至少证明：

## Map

无需 QGIS 人工编辑即可从一个代表性城市自动生成基本正确 routing graph。

## Signal

明确 semaphore clock domain 和 phase anchor 机制。

如果目标为 countdown，则：

\[
|e|\le1s.
\]

## Telemetry

连续运行数小时：

- 不导致 ETS2 崩溃；
- 数据连续；
- pause/load 正确。

## Performance

Debug core 对 ETS2 frametime 无显著影响。

---

# 76. 对此前方案的最终审核结论

此前方案以下部分保持不变：

- PC Core；
- Mobile Renderer；
- 自主路线；
- 多路线；
- Map Matching；
- 偏航；
- POI；
- Junction View；
- 中文 TTS；
- GLOSA；
- 官方 DLC 本地初始化；
- MapLibre/PMTiles；
- WebSocket；
- Rust Navigation Core。

以下部分已经修正：

### 修正 1

从：

> “红绿灯 profile + simulation timestamp 可以计算 phase”

改为：

> “首先通过实验确定 clock domain 和 phase anchor，再决定纯计算或 hybrid。”

### 修正 2

从：

> “城市 1:3、高速 1:19 必须直接换算 semaphore interval”

改为：

> “目前证据不足，必须实验确定 semaphore timer 与地图时间压缩的关系。”

### 修正 3

从：

> “自动地图解析后基本即可自动 routing”

改为：

> “Routing Graph Builder 是独立的 P0 核心工程。”

### 修正 4

从：

> “兼容格式的 Mod 自动支持”

改为：

> “未知 Mod 仅进行实验性 best-effort parsing，不属于兼容承诺。”

### 修正 5

从：

> “只有 SDK 变化才需软件更新”

改为：

> “SDK、地图文件格式、资源结构或语义变化均可能要求解析器更新。”

### 修正 6

从：

> “全项目 Rust”

改为：

> “C++ Telemetry + Rust Runtime + C#/TS Map Compiler 的多语言结构。”

---

# 77. 最终架构判断

本项目仍然具备现实可行性。

现有公开技术证据已经足以支持：

```text
Telemetry
地图读取
官方 DLC
地图显示
自主路径规划
移动端第二屏
Map Matching
偏航
限速
POI
TTS
```

TruckNav 已经实际实现独立 routing、实时 telemetry、Desktop/Android/Browser，而 TruckSim Maps 已经能够从官方 DLC 自动解析地图并输出地图资源，因此基础架构不是理论设想。

真正需要研发突破的是两个问题：

\[
\boxed{\text{Automatic Semantic Routing Graph}}
\]

以及

\[
\boxed{\text{Traffic Signal Runtime Phase}}
\]

其中：

> 前者决定整个系统是否能够在地图更新后真正做到“自动重新生成导航路网”；

> 后者决定红绿灯倒计时、即将绿灯提醒和 GLOSA 是否能够满足既定 ±1 s 精度。

因此项目当前最合理的工程路线不是继续扩展功能，而是首先完成：

```text
        ┌─ Routing Graph P0
Raw Map ┤
        └─ Semaphore P0

            ↓

两个风险得到定量结果

            ↓

       完整 Navigation Core

            ↓

         高德式 UI
```

在这两个 P0 结果出来之前，不应投入大量时间制作最终 UI。

这就是审核后的正式 v0.2 技术基线。