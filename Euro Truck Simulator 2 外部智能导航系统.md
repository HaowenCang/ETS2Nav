# Euro Truck Simulator 2 外部智能导航系统
## 第一版需求规格与技术路线
版本：v0.1  
状态：需求基线 / 技术预研方案

---

# 1. 项目定义

本项目拟为 Euro Truck Simulator 2（ETS2）开发一套完全独立于游戏原生 Route Advisor 的外部智能导航系统。

系统不读取或复用游戏已经规划完成的导航路线，而是自行从 ETS2 地图资源中建立道路网络、路口结构、交通信号灯、限速、POI 等地图数据库，并在此基础上完成：

- 独立路径规划；
- 多路线比较；
- 实时车辆定位和地图匹配；
- 转向导航；
- 红绿灯倒计时及驾驶辅助；
- 超速及测速提示；
- POI 搜索；
- 偏航重新规划；
- 休息站、加油站规划；
- 类似现代高德地图的导航 UI。

系统原则上完全本地运行，不依赖互联网地图服务或云端导航服务。

核心导航程序运行于 ETS2 所在 PC，显示层与导航核心解耦：

```text
ETS2
 │
 │ SCS Telemetry SDK
 ▼
Telemetry Bridge
 │
 ▼
Navigation Core
 ├── Map Matcher
 ├── Routing Engine
 ├── Traffic Light Engine
 ├── Guidance Engine
 ├── Warning Engine
 └── Search / POI Engine
 │
 ├──────── PC UI
 │
 └──────── LAN API ────── Mobile UI
```

PC 可以仅运行核心服务而不进行地图图形渲染。移动设备通过局域网获取已经处理好的导航状态并负责 UI 渲染。

---

# 2. 平台与兼容范围

## 2.1 正式支持范围

V1 正式支持：

- Euro Truck Simulator 2；
- ETS2 官方基础地图；
- 用户已经安装的官方地图 DLC；
- Windows PC；
- 单人模式。

明确不支持：

- TruckersMP；
- ATS；
- 对特定第三方地图 Mod 进行人工适配；
- 高德、Google Maps 等现实世界地图数据作为道路规划基础。

ATS 可以保留架构兼容能力，但不属于 V1 产品范围。

---

# 3. 地图 Mod 兼容原则

系统不得针对 ProMods、RusMap 等地图 Mod 单独维护专用路线图。

第三方地图 Mod 采用“结构兼容即自动支持”的原则。

如果某 Mod：

1. 使用系统已经能够解析的 SCS 地图 sector；
2. 使用能够解析的 Road、Prefab、Navigation Point、Sign、Semaphore 等数据结构；
3. 没有引入导航核心无法理解的自定义语义；
4. 地图拓扑通过完整性检查；

则系统应当在地图初始化过程中自动读取该 Mod，并生成新的导航图，无需开发者针对该 Mod 编写任何适配代码。

否则该 Mod 被标记为：

- Compatible；
- Partially Compatible；
- Unsupported。

不得通过猜测方式建立不可靠路线。

系统必须正确处理 Mod 的资源覆盖顺序，否则某些地图 Mod 即使格式兼容，也可能因为资源解析顺序与 ETS2 实际加载顺序不同而生成错误地图。

---

# 4. 地图更新机制

用户首次安装导航软件后执行地图初始化：

```text
ETS2 archives
      ↓
资源扫描
      ↓
地图/定义文件解析
      ↓
道路拓扑构建
      ↓
交通规则传播
      ↓
POI / 信号灯 / 摄像头提取
      ↓
路线图生成
      ↓
空间索引
      ↓
地图矢量瓦片
      ↓
Navigation Database
```

地图数据库必须由用户本机 ETS2 文件生成，不随软件发行固定版本的欧洲路线图。

现有 TruckSim Maps 已经证明可以直接解析 ATS/ETS2 游戏文件并生成 JSON、GeoJSON、PMTiles 等地图资源，而且支持官方 DLC；其作者指出完整解析通常需要数分钟。

系统启动时计算地图资源 fingerprint，例如：

```text
GameVersion
DLC list
archive metadata
sector hashes
definition hashes
enabled map mods
mod load order
```

若 fingerprint 未变化，则直接加载缓存。

若地图内容变化，则自动重新构建。

V1 可以采用：

> 地图发生变化 → 完整重新构建

V2 再实现：

> sector hash → 仅重新解析发生变化的 sector → 增量更新路线图

需要特别说明：不能保证“只有 Telemetry SDK 更新才需要升级本软件”。

SCS 官方文档本身明确提示游戏数据结构可能随游戏更新发生显著变化；甚至官方 Game Archive Extractor 在 1.55 后也已经使用新的版本。因此，如果 SCS 修改地图二进制格式、资源结构或字段语义，即使 Telemetry SDK 本身没有改变，解析器仍可能需要升级。

因此应当定义为：

> 普通地图内容更新应自动兼容；地图数据格式发生不兼容改变时允许要求软件更新。

---

# 5. 数据结构设计

系统内部应当至少建立两层道路网络，而不是只建立一个简单道路图。

## 5.1 Routing Graph

用于长距离路径规划。

节点主要包括：

- road endpoint；
- prefab control node；
- junction entry/exit；
- ferry/train connection；
- company entrance；
- POI access point。

有向边保存：

```text
edge_id
from
to
geometry
length
road_class
speed_limit
country
city
road_number/name
toll
ferry/train
traffic_lights[]
speed_cameras[]
rest_area
fuel_station
construction
turn_metadata
```

建议使用紧凑 CSR/adjacency-array 数据结构，而不是运行时大量对象，以降低内存占用并提高路径搜索缓存效率。

## 5.2 Junction / Lane Graph

用于：

- 信号灯关联；
- 精确路口转向；
- 环岛出口；
- 2D 路口放大图；
- 未来车道级导航。

SCS Prefab 中的 Navigation Point 本身描述 AI 行驶路径，而且 Traffic Semaphore ID 与 Navigation Point 直接关联；因此可以由这一层判断“当前车辆所在入口方向具体受哪一组信号灯控制”。

官方 Prefab 数据也明确区分 Map Point 与 Navigation Point：Map Point 用于世界地图和 GPS 导航，Navigation Point 用于具体 AI 行驶路径。

因此车道级导航从数据模型上具有实现基础，但其工程复杂度明显高于普通导航，应放入 V2。

---

# 6. 路线规划

## 6.1 基本原则

路线必须由本软件自主计算。

不得调用游戏原生导航路线作为路径规划结果。

系统默认生成 2～3 条具有明显策略差异的路线，例如：

- 时间优先；
- 距离优先；
- 综合推荐。

具体策略在后续调参阶段确定。

---

# 7. 路线代价模型

对于道路边 \(e\)，基础通行时间定义为

\[
T_e=\frac{L_e}{V_e}
\]

其中：

- \(L_e\)：道路长度；
- \(V_e\)：预计实际行驶速度。

综合代价可以写为

\[
C_e =
w_tT_e+
w_dL_e+
w_sS_e+
w_rR_e+
w_pP_e .
\]

其中：

- \(S_e\)：红绿灯预计等待成本；
- \(R_e\)：道路类型、城市道路、复杂转向等成本；
- \(P_e\)：收费、渡轮等附加代价。

不得简单使用“每个红绿灯增加固定 30 秒”。

如果可以读取信号灯周期，则应根据：

- 绿灯持续时间；
- 黄灯持续时间；
- 红灯持续时间；
- cycle offset；

估算随机到达该路口时的统计平均延误。

因此即使路线计算时不知道车辆真正抵达路口的准确相位，也能判断：

> 经过 18 个城市信号灯的路线

通常应比：

> 同距离但仅经过 4 个信号灯的路线

具有更高预期时间成本。

---

# 8. 多路线生成

不建议单纯计算传统 K-shortest paths，因为这种方法容易得到几条几乎完全重叠的路线。

推荐采用：

```text
Profile A：Fastest
Profile B：Shortest
Profile C：Balanced
         ↓
分别计算最优路径
         ↓
路径重叠度分析
         ↓
去除高度重复方案
         ↓
必要时施加 overlap penalty 重新规划
```

最终最多显示 3 条具有实际差异的路线。

每条路线应显示：

- 距离；
- 预计行驶时间；
- 红绿灯数量；
- 高速/普通道路比例；
- 收费设施；
- 渡轮/火车等关键差异。

其中预计行驶时间即使暂不作为 UI 核心功能，也必须在路由引擎内部计算，因为“时间优先”无法脱离 ETA 模型实现。

---

# 9. 路由算法

V1 推荐：

> A* + 高质量 heuristic + 预处理索引

道路图规模扩大后可升级为：

- ALT（A* + Landmarks）；
- Contraction Hierarchies；
- Customizable Contraction Hierarchies。

由于不同路线策略需要动态改变权重，不能过早使用只适用于完全固定权重的预处理方案。

第一阶段应先保证路线正确，再优化到：

> 欧洲任意两个地点之间 2～3 条路线均能在亚秒到低秒级完成。

该数值属于性能设计目标，需要通过最终地图规模和硬件基准测试确定，而不是当前阶段的既定事实。

---

# 10. 实时车辆数据

SCS 官方 Telemetry SDK 提供玩家车辆遥测接口，当前官方文档列出的稳定 SDK 为 1.14。

建议自行实现轻量 `scs-nav-bridge.dll`，而不是将第三方 telemetry server 作为强依赖。

插件只负责：

```text
position
orientation
speed
game/simulation timestamp
current speed limit
job information
fuel
rest-related state
pause state
```

并通过：

- Shared Memory；
- 或 Named Pipe

发送给 Navigation Core。

现有 SCS 插件实践已经证明通过 memory-mapped file 传输 Telemetry 数据是成熟方案，并且不会产生持续硬盘 I/O。

---

# 11. 当前货运任务目的地

系统需要支持：

> 直接将当前货运任务目的地设置为导航目的地。

现有基于 SCS SDK 的实现可以取得：

- destination city；
- destination city ID；
- destination company；
- destination company ID；

因此可以使用 destination company ID 与本地地图 POI 数据库进行匹配。

流程为：

```text
Telemetry job destination
        ↓
company ID + city ID
        ↓
POI database lookup
        ↓
company entrance / destination point
        ↓
Route Planning
```

如果一个公司具有多个入口，应当导航至实际可进入的入口节点，而不是公司几何中心。

---

# 12. Map Matching

Telemetry 返回车辆世界坐标后，不应简单寻找最近道路。

算法至少应考虑：

\[
P(e|x) =
f(
distance,
heading,
previousEdge,
topology,
speed
)
\]

即同时利用：

- 车辆与道路距离；
- 车辆航向；
- 上一时刻所在 road edge；
- 道路连接关系；
- 当前运动方向。

这样可以避免在：

- 上下层立交；
- 高速公路双向车道；
- 平行辅路；
- 环岛；

附近跳到错误道路。

可以采用 HMM/Viterbi Map Matching，也可以使用针对 ETS2 高频连续坐标优化的递归 candidate tracking。

---

# 13. 偏航检测与重新规划

偏航重新规划属于 V1 必选功能。

不得因为 GPS 坐标短暂偏离道路立即重新规划。

应区分：

```text
正常定位噪声
短暂驶离中心线
停车场内部移动
真正驶入非规划道路
```

建议连续多帧确认：

\[
P(\text{off-route}) > P_\mathrm{threshold}
\]

或车辆已经沿新 edge 行驶超过一定距离后，才确定偏航。

偏航确认后：

```text
当前 map-matched edge
        ↓
新起点
        ↓
重新运行当前 route profile
        ↓
生成新 maneuver list
```

目标为偏航确认后约 1 秒量级完成新的导航路线。

---

# 14. 红绿灯系统

这是本项目最高技术风险模块，也是开发前必须完成的 P0 原型。

SCS Prefab Traffic Semaphore 数据明确包含：

- semaphore ID；
- Profile；
- Green interval；
- Orange interval；
- Red interval；
- Orange interval；
- Cycle Delay。

官方资料还说明 semaphore profile 保存周期和 cycle shift。

因此：

> “每一组信号灯的理论周期”

可以从地图资源获得。

真正需要验证的是：

> 如何从当前游戏 simulation timestamp 精确恢复该 semaphore 在此刻处于周期中的哪个位置。

---

# 15. 红绿灯精度要求

对于系统声明支持实时倒计时的标准信号灯：

\[
|t_\mathrm{shown}-t_\mathrm{actual}|\leq1.0~\mathrm{s}
\]

为硬性验收条件。

这里采用绝对误差上限，而不是平均误差。

如果同步置信度不足，则应：

> 只显示“红灯/绿灯”，不显示具体秒数；

而不是显示可能偏差数秒的倒计时。

---

# 16. 红绿灯同步研究方案

P0 首先验证：

```text
Semaphore profile
+
cycle offset
+
simulation timestamp
        ↓
phase(t)
        ↓
current state
+
remaining time
```

必须专门测试：

- 正常游戏速度；
- 游戏暂停；
- 快速旅行之后；
- 读取存档之后；
- 不同城市；
- 不同国家；
- 多种 prefab；
- 红黄绿不同 profile；
- 连续多个游戏 session。

建议编写自动日志工具，同时记录：

```text
simulation timestamp
semaphore ID
predicted state
predicted remaining time
```

并利用屏幕录像逐帧记录实际信号灯变化时刻。

至少覆盖数百次状态转换。

只有在最大绝对误差能够稳定控制在 1 s 内，才将该信号灯类型标记为：

`COUNTDOWN_SUPPORTED`

否则标记：

`STATE_ONLY` 或 `UNSUPPORTED`。

---

# 17. 前方信号灯识别

不能简单按照“最近的信号灯”决定提醒对象。

必须：

```text
当前 Routing Edge
      ↓
下一个 Junction
      ↓
当前进入 Junction 的 Navigation Lane
      ↓
Traffic Semaphore ID
      ↓
Semaphore Profile
```

这样才能区分同一路口的：

- 直行灯；
- 左转方向；
- 对向信号灯；
- 横向道路信号灯。

SCS 的 Navigation Point 与 Traffic Semaphore ID 存在明确绑定关系，因此这一算法具有地图数据基础。

---

# 18. 红灯减速提醒

当车辆驶向红灯时，系统计算停车距离：

\[
d_\mathrm{stop}
=
v\,t_r+
\frac{v^2}{2a}
+
d_\mathrm{margin}.
\]

其中：

- \(v\)：当前速度；
- \(t_r\)：驾驶者反应时间模型；
- \(a\)：保守减速度；
- \(d_\mathrm{margin}\)：安全余量。

如果

\[
d_\mathrm{signal}\lesssim d_\mathrm{stop},
\]

则触发：

> 前方红灯，请减速。

V1 不应假装精确计算实际卡车最大制动距离，因为实际制动能力受车辆、挂车、载重、道路条件和游戏物理设置影响。

因此该功能定位为：

> 提醒系统，而非自动紧急制动模型。

---

# 19. 即将绿灯提醒

若满足：

- 当前为红灯；
- 倒计时可靠；
- 剩余时间低于设定阈值；
- 车辆低速或静止；

则允许提示：

> 即将绿灯。

默认阈值后续通过实车游戏测试确定，例如 3～5 s。

---

# 20. GLOSA 绿灯建议速度

系统加入 Green Light Optimal Speed Advisory。

假定距停止线距离为 \(d\)，下一段绿灯窗口相对于当前时刻为

\[
[t_1,t_2].
\]

希望车辆抵达时间满足：

\[
t_1\leq \frac d v\leq t_2.
\]

因此可行速度区间为

\[
\frac d{t_2}\leq v\leq\frac d{t_1}.
\]

再与：

- 当前道路限速；
- 下一段道路限速；
- 可实现加减速度；
- 最低合理速度；

求交集。

最后将区间量化到 5 km/h 或 10 km/h：

例如数学结果：

\[
53.7\leq v\leq63.1\;\mathrm{km/h}
\]

UI 显示为：

> 建议 55–60 km/h

而不是：

> 58.4 km/h。

因此降低显示精度不会显著降低计算复杂度——这一计算本身几乎没有性能压力——其主要价值是使提示更符合实际驾驶行为并减少驾驶者频繁调整速度。

如果不存在合理速度区间，则不显示 GLOSA。

---

# 21. 限速系统

系统显示：

- 当前速度；
- 当前限速；
- 前方限速变化。

当前限速可以同时使用：

1. 自建地图数据库；
2. SCS Telemetry SDK 当前 speed limit。

现有 SDK 实现中已经存在实时 Speed Limit 字段。

这两个来源应互相校验。

若：

```text
ParsedMapLimit != TelemetrySpeedLimit
```

则记录诊断信息，用于修正地图解析规则。

SCS 官方资料指出现代地图中的 speed limit 主要由 signs 管理，而不是简单依赖 Navigation Point traffic rule。

因此地图初始化必须正确解析：

- 国家默认限速；
- 道路类型；
- 限速标志；
- 限速解除；
- 城市区域；
- 特殊道路规则。

不能简单从 road type 推测全部限速。

---

# 22. 超速提醒

允许用户配置：

- 提醒阈值；
- 声音；
- 是否重复提醒；
- 显示方式。

建议默认采用：

```text
≤ 50 km/h：limit + 3 km/h
> 50 km/h：limit + 5 km/h
```

这里只作为待测试默认方案，最终设置应当允许用户改变。

---

# 23. 测速摄像头

V1 目标：

- 在初始化地图时识别固定测速点；
- 保存方向、位置和对应道路；
- 行驶方向匹配后才提醒；
- 显示对应限速；
- 用户可配置提醒距离。

例如：

> 前方 500 m 测速  
> 限速 80

如果已经超速，提高提示优先级。

测速点具体在官方地图中的完整编码方式应纳入地图解析 P0/P1 调研，不应在验证前假设全部摄像头都能仅靠某一种模型名称识别。

---

# 24. 导航转向指令

系统自行根据 Route Geometry 生成 maneuver。

至少支持：

- 左转；
- 右转；
- 轻微左转；
- 轻微右转；
- 掉头；
- 直行；
- 靠左；
- 靠右；
- 高速驶入；
- 高速驶出；
- 环岛。

转向类型由：

- incoming heading；
- outgoing heading；
- junction topology；
- road class；

共同决定，而不是简单按夹角分类。

---

# 25. 2D 路口放大图

V1 实现类似高德地图的 Junction View。

但不使用预制截图。

由导航图实时生成矢量示意：

```text
Junction topology
       ↓
抽取附近道路
       ↓
geometry simplification
       ↓
突出规划路线
       ↓
SVG / Canvas rendering
```

优点是：

- 自动适配任何官方路口；
- 地图更新后无需人工制作图片；
- 可动画；
- 可以直接扩展到环岛和复杂立交。

临近路口时自动放大，驶离后恢复普通导航视图。

---

# 26. 环岛导航

必须识别环岛环形拓扑并计算：

> 第 N 个出口。

不得简单将环岛拆成多个连续“右转”。

需要从车辆进入环岛的连接点开始，对规划路线经过的可驶出 branch 计数。

---

# 27. 高速出口编号

目标功能：

> 前方 X km 从 XX 出口驶出。

但此功能依赖地图中路牌、道路编号或 sign template 是否可以稳定解析。

因此列为：

> V1 目标功能，但必须经过 sign parser 覆盖率验证。

若地图没有可靠 exit number，则退化为：

> 前方出口驶出 / 前往 XXX 方向。

不得自行生成不存在于地图中的编号。

---

# 28. 导航播报系统

导航播报距离不能仅固定为一组常量。

基础模型：

\[
D_\mathrm{prompt}
=
f(
v,
roadClass,
maneuverComplexity
).
\]

然后由用户选择播报频率。

例如：

低频：

```text
2 km
500 m
100 m
转向
```

标准：

```text
2 km
1 km
500 m
250 m
100 m
转向
```

高频可以增加更多节点。

高速路的提醒应整体提前，低速城市道路提醒应适当后移。

---

# 29. 中文语音导航

V1 中文语音为正式功能。

Navigation Core 不直接生成自然语言音频，而生成语义指令：

```text
TURN_RIGHT
distance=500
road=E40
exit=12
```

Speech Engine 再转换为：

> 前方五百米右转。

这样可以轻易扩展：

- 中文；
- 英文；
- 不同播报频率；
- 不同措辞。

V1 可优先调用 Windows / Android 系统 TTS。

如果后续需要统一高质量声音，再评估离线神经网络 TTS。

---

# 30. POI

必须建立本地 POI 数据库。

V1 POI：

- 城市；
- 公司；
- 维修站；
- 车库；
- 加油站；
- 服务区；
- 休息站；
- 渡轮；
- 火车运输点；
- 收费站；
- 国境检查设施。

用户应当支持：

```text
搜索城市
搜索公司
搜索 POI
```

并可直接开始导航。

---

# 31. 加油规划

V1 加入基础加油站规划。

系统知道：

- 当前 fuel；
- telemetry fuel range；
- 路线上加油站。

如果：

\[
\text{RemainingRouteDistance}
>
\text{EstimatedRange}-SafetyMargin,
\]

则提示沿途加油。

用户也可以主动搜索：

> 沿途加油站

并计算绕行距离。

不实现复杂经济模型或油价优化。

---

# 32. 休息站规划

V1 加入休息站导航。

系统应允许：

- 搜索最近休息站；
- 搜索沿途休息站；
- 将休息站作为 waypoint 插入当前路线。

如果能够可靠取得疲劳相关 Telemetry，则可以提示：

> 建议在前方休息站休息。

但不加入货物交付时间风险模型。

---

# 33. 其他道路提示

V1 需求包括：

- 收费站；
- 渡轮；
- 火车；
- 国境线；
- 检查站；
- 道路施工；
- 临时限速。

需要区分：

### Static Map Event

地图文件固定存在的：

- 收费站；
- 国境；
- 固定施工道路；
- 固定临时限速；

可以直接支持。

### Dynamic Random Event

游戏运行过程中随机生成的：

- 动态事故；
- 临时施工；
- 随机封路；
- 随机事件车辆；

如果官方接口无法取得其位置和状态，则 V1 不保证实时识别。

不得把“可以解析静态 road event”错误表述成“可以读取所有游戏实时道路事件”。

---

# 34. 自动缩放

导航地图缩放级别根据：

- 当前速度；
- 下一个 maneuver 距离；
- maneuver 类型；

自动调整。

高速巡航：

> 缩小地图，显示更远路线。

接近路口：

> 自动放大。

复杂路口：

> 切换 Junction View。

用户手动操作地图后允许暂时关闭自动缩放，并在若干秒后恢复。

---

# 35. 当前道路名称

系统尽可能显示：

- road name；
- motorway number；
- European route number；
- city road information。

但 ETS2 本身并非每段道路都有现实世界意义上的“道路名称”。

因此 fallback：

```text
正式道路名称
    ↓
道路编号
    ↓
道路类别
    ↓
不显示
```

不得自行杜撰道路名称。

---

# 36. PC Core 架构

建议采用：

### Navigation Core

Rust。

原因：

- 二进制地图解析安全性较高；
- 性能稳定；
- 内存控制良好；
- 并发和后台服务方便；
- 很适合实现 graph、spatial index 和 WebSocket server。

Telemetry Plugin：

- C++17；
- 或 Rust C ABI。

为了降低第一阶段风险，可以采用：

> C++ Telemetry Bridge + Rust Navigation Core。

---

# 37. PC 前端与移动端

推荐共享 Web 技术栈：

```text
Vue / React
+
TypeScript
+
MapLibre GL
```

Windows：

> Tauri Desktop Shell

移动端：

> PWA / Capacitor App

这样同一套 UI 可以运行在：

- PC 窗口；
- 第二显示器；
- Android；
- iPad；
- 浏览器。

现有 TruckNav 已经证明 ETS2 实时位置、独立 routing、Desktop、Android 和 Browser 的组合在工程上可行。

但本项目的路线图应完全自行生成，而不采用其预先人工修正后的地图数据库。

---

# 38. 地图渲染

建议本地地图编译成 vector tiles，例如：

> PMTiles。

显示层使用 MapLibre。

道路和路线的距离计算始终在 ETS2 原生坐标系中完成。

地图 renderer 可以将 ETS2 坐标投影至一个内部平面地图坐标系，仅用于视觉显示。

不要将“转换成现实 WGS84 坐标”作为导航计算的必要步骤，因为本项目只处理 ETS2 世界，不需要与现实 GPS 数据叠加。

---

# 39. PC 与移动端通信

使用：

### HTTP

用于：

- POI 搜索；
- route request；
- settings；
- map metadata；
- tiles。

### WebSocket

用于实时状态：

```text
vehicle
navigation
maneuver
traffic_light
speed_limit
warning
route_progress
```

推荐 10～20 Hz 更新。

示例：

```json
{
  "vehicle": {
    "speed": 83.2,
    "speedLimit": 80
  },
  "navigation": {
    "remainingDistance": 142300,
    "nextManeuverDistance": 487
  },
  "trafficLight": {
    "state": "RED",
    "remaining": 12.4,
    "confidence": 0.99
  }
}
```

UI 可以以 60 FPS 插值车辆动画，而不要求后台 60 Hz 发送网络数据。

---

# 40. 局域网连接

用户打开 PC 程序后显示：

```text
192.168.x.x:xxxx
```

以及二维码。

移动端扫描二维码完成连接。

二维码携带随机 session token。

服务默认只监听：

- localhost；
- 私有 LAN interface。

不得默认暴露到公网。

---

# 41. 性能设计

PC 运行 Navigation Core 本身不应成为显著性能瓶颈。

持续执行的主要工作只有：

```text
Telemetry read
20–50 Hz
     ↓
Map Matching
     ↓
Route progress
     ↓
Traffic Light / Warning evaluation
     ↓
WebSocket update
```

这些任务计算规模远小于 ETS2 的图形渲染和物理模拟。

真正较重的是：

- 首次地图解析；
- 地图更新后数据库重建；
- vector tile 生成。

因此规定：

> 游戏运行时禁止自动执行完整地图重建。

发现地图变化时：

- 游戏未运行 → 可以初始化；
- 游戏运行 → 使用现有数据库并提示更新，或低优先级延后重建。

---

# 42. 性能目标

以下作为工程验收目标，而不是当前已经测得的数据。

参考现代 6 核以上 CPU：

Navigation Core steady state：

- CPU：尽量 < 3% 系统总 CPU；
- RAM：目标 < 300～500 MB；
- 不产生持续高频磁盘写入。

Route Planning：

- 单路线典型 < 500 ms；
- 三路线典型 < 1 s；
- 极端情况 < 2 s。

Rerouting：

- 偏航确认后 < 1 s。

Telemetry → Core：

- < 50 ms。

PC Core → LAN UI：

- 目标端到端状态延迟 < 200 ms。

Navigation UI：

- 目标 60 FPS。

这些指标必须在：

> ETS2 单独运行

和

> ETS2 + Navigation Core

两种情况下对照测试 1% low FPS、平均 FPS、CPU frametime 和 GPU frametime。

只有帧率和 frametime 测量才能判断是否“不会影响正常游戏”。

---

# 43. 为什么移动端渲染更适合性能敏感场景

如果采用：

> PC Core + Mobile Renderer

PC 不需要执行：

- MapLibre WebGL；
- UI 动画；
- 地图纹理渲染。

PC 只进行 CPU 侧导航计算。

因此这是性能敏感情况下推荐的默认工作模式。

PC UI 仍然保留，但可选择关闭。

---

# 44. UI 产品目标

设计目标：

> 信息架构、交互逻辑、导航状态切换和动画表现尽可能接近现代高德地图导航。

包括：

- 顶部转向卡片；
- 下一 maneuver；
- 距离；
- 当前速度；
- 限速标牌；
- 红绿灯倒计时；
- 路线高亮；
- 自动缩放；
- 路口放大；
- 底部剩余距离等状态；
- 平滑车辆跟随；
- 昼夜导航样式。

但正式软件应自行制作：

- icon；
- vector asset；
- animation；
- color system；
- typography；
- map style。

不能直接打包高德地图私有 UI 资源。

---

# 45. V1 功能范围

V1 必须完成：

- 本地自动地图解析；
- 官方 DLC 自动识别；
- Routing Graph；
- 独立路线规划；
- 2～3 条不同策略路线；
- Map Matching；
- 实时车辆位置；
- 当前速度；
- 当前限速；
- 前方限速；
- 超速提醒；
- 测速提醒；
- 转向提示；
- 2D 路口图；
- 环岛出口；
- 剩余距离；
- 当前道路；
- 自动缩放；
- 路口自动放大；
- 中文 TTS；
- 偏航重规划；
- 城市/公司/POI 搜索；
- 当前货运任务目的地；
- 服务区；
- 加油站；
- 休息站；
- 收费站；
- 渡轮/火车；
- 国境/检查站；
- 静态施工和临时限速；
- 红绿灯识别；
- 红绿灯倒计时；
- 红灯减速提示；
- 即将绿灯提醒；
- GLOSA 建议速度；
- PC UI；
- LAN Mobile UI。

---

# 46. V2 功能候选

V2：

- Lane Graph；
- 车道级导航；
- 高级高速分叉；
- 更复杂 Junction View；
- 增量地图更新；
- 更完善 Mod compatibility diagnostics；
- 多 waypoint；
- 路线拖拽；
- 更高质量离线 TTS。

---

# 47. 车道级导航复杂度评估

技术可行性：

> 中高。

实现难度：

> 高于普通路径规划，但并非缺乏基础数据。

原因是 SCS Navigation Point 本身保存：

- AI lane；
- allowed vehicles；
- blinker；
- priority；
- semaphore association；
- boundary lane；

这些数据有能力描述 prefab 内部具体车流路径。

真正困难的是：

> 将 prefab 内部 lane 与 prefab 外部 road lane 连续、稳定地拼接成完整 lane graph。

因此 V1 数据模型应保留 lane 信息，但不在 UI 中提供车道级提示。

这样 V2 不需要重新设计地图数据库。

---

# 48. 核心技术风险排序

## Risk 1：Traffic Light Runtime Synchronization

风险：最高。

目标：

\[
|e|\leq1~s.
\]

必须先验证。

如果失败，本项目仍能完成绝大多数导航功能，但不能达到既定红绿灯倒计时要求。

---

## Risk 2：Automatic Routing Graph Generation

现有开源项目已经证明地图可以自动解析，但也说明 prefab、路口、环岛等拓扑容易产生错误；TruckNav 作者甚至曾需要大量 QGIS 和脚本人工修复 routing graph。

本项目要求“更新地图后完全自动生成”，因此必须比现有 demo 更重视 graph validation。

至少实现：

```text
dead-end detection
illegal U-turn detection
one-way validation
isolated component detection
prefab connectivity validation
roundabout validation
company entrance validation
```

---

## Risk 3：Speed Limit Propagation

不能只读取道路类型。

需要正确恢复 signs 作用后的限速状态。

可以利用 Telemetry 当前 Speed Limit 作为 ground truth 对 parser 进行大量自动验证。

---

## Risk 4：Dynamic Road Events

官方 Telemetry 不一定暴露所有随机事件状态。

如果无法取得，则只支持地图静态事件。

---

## Risk 5：Highway Exit / Road Names

受地图文本和 sign 数据完整度限制。

应该允许 graceful degradation。

---

# 49. 推荐开发顺序

## P0 — 可行性验证

不做完整 UI。

只解决最危险的问题：

### P0-A

读取：

- position；
- heading；
- speed；
- simulation timestamp。

### P0-B

解析一个城市：

- roads；
- prefab；
- Navigation Points；
- Traffic Semaphores。

### P0-C

构建一个局部 Junction Graph。

### P0-D

定位车辆当前道路和前方信号灯。

### P0-E

计算信号灯当前状态与倒计时。

重点验收：

\[
|e|\leq1~s.
\]

这是整个项目的第一个 Go / No-Go 技术门。

---

# 50. P1 — 地图编译器

完成：

```text
Game Archive Reader
Resource Resolver
Sector Parser
Prefab Parser
SII Parser
Sign Parser
Semaphore Parser
POI Parser
Graph Builder
Graph Validator
Tile Generator
Search Index
```

输入：

> 用户 ETS2 安装目录。

输出：

```text
map.db
routing.graph
junction.graph
search.db
map.pmtiles
metadata.json
```

---

# 51. P2 — 基础导航

完成：

- Map Matching；
- A*；
- Fastest / Shortest / Balanced；
- route geometry；
- maneuvers；
- remaining distance；
- deviation detection；
- rerouting。

此阶段使用简单 debug map 即可。

---

# 52. P3 — 驾驶辅助

加入：

- speed limit；
- speed warning；
- camera warning；
- traffic light countdown；
- red-light braking warning；
- green-light warning；
- GLOSA。

---

# 53. P4 — UI

实现高德式导航界面：

```text
地图
车辆标记
路线
下一动作
距离
速度
限速
红绿灯
Junction View
POI
路线选择页
```

先实现 PC/browser UI。

然后封装 Android。

---

# 54. P5 — 完整路线测试

建立自动化测试集。

至少覆盖：

- 欧洲跨国长距离；
- 城市内部；
- 高速互通；
- 环岛；
- 公司内部；
- 渡轮；
- 火车；
- 收费站；
- 国境；
- 服务区；
- 加油站；
- 休息站。

随机生成数千组 origin-destination route，自动检查：

- graph connectivity；
- illegal edge；
- U-turn；
- impossible junction；
- discontinuous geometry。

---

# 55. P6 — 性能优化

只有正确性达到要求之后再做：

- graph compression；
- spatial R-tree；
- ALT/CH；
- binary serialization；
- memory mapping；
- incremental map compilation；
- WebSocket binary protocol。

不建议在早期为了追求路线查询几十毫秒而提前复杂化架构。

---

# 56. 推荐仓库结构

```text
ets2-nav/
│
├─ scs-bridge/
│   └─ Telemetry SDK plugin
│
├─ map-compiler/
│   ├─ archive/
│   ├─ sector/
│   ├─ sii/
│   ├─ prefab/
│   ├─ sign/
│   ├─ semaphore/
│   ├─ graph/
│   └─ tiles/
│
├─ nav-core/
│   ├─ map_matching/
│   ├─ routing/
│   ├─ maneuver/
│   ├─ traffic_light/
│   ├─ warnings/
│   ├─ poi/
│   └─ tts/
│
├─ nav-server/
│   ├─ http/
│   └─ websocket/
│
├─ ui/
│   ├─ map/
│   ├─ navigation/
│   ├─ junction/
│   ├─ route-selection/
│   └─ settings/
│
└─ tests/
    ├─ map/
    ├─ routing/
    ├─ semaphore/
    └─ performance/
```

---

# 57. 最终技术判断

项目整体技术可行。

目前具有充分依据认为以下部分能够完成：

- PC 获取 ETS2 Telemetry；
- 本地解析官方地图；
- 官方 DLC 自动建立地图数据库；
- 自主路径规划；
- 多路线；
- 实时车辆位置；
- Map Matching；
- POI 搜索；
- 超速提醒；
- 当前限速；
- 2D 路口导航；
- 偏航重规划；
- PC + LAN Mobile 架构。

SCS Prefab 数据已经明确包含 GPS navigation、AI navigation lane 和 Traffic Semaphore profile 等结构，因此项目的数据基础明显优于纯视觉识别方案。

当前唯一不能在未经实验的情况下宣称已经满足需求的是：

> **实时红绿灯剩余时间 ±1 s。**

地图已经提供信号灯周期和 cycle 信息，但必须进一步确认游戏运行时相位与 simulation timestamp 的精确对应关系。

因此本项目正确的工程顺序不是先制作高德式 UI，而是：

```text
Telemetry
   ↓
地图解析
   ↓
Semaphore runtime synchronization
   ↓
±1 s 验证
   ↓
Routing Graph
   ↓
Route Engine
   ↓
Navigation Engine
   ↓
UI
```

其中 **P0 红绿灯同步实验与 P1 自动路线图生成器** 是整个项目最关键的两个技术模块。