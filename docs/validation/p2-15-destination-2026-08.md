# P2-15 Destination Resolver 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §107-110（统一模型/POI access_node/Job 映射/失败语义）。
**产物**：nav-router destination 模块 + nav-dataset load_pois（rusqlite）+ CLI `dest` 命令。

## 一、实现

### Destination 模型（§107）
```rust
Destination { kind: Poi|Job|MapClick|Coordinate, name, position(视觉), access_snap(导航目标) }
DestError { NotFound, Ambiguous }
```

### POI 解析（§108）
- search.db poi 表（rusqlite bundled——`load_pois`）
- **access_node 作为正式导航目标**（不用 visual position）
- 匹配：精确 → 包含（不区分大小写）；多命中 Ambiguous

### Job 目的地（§109）
- `resolve_job(company, city_hint)`：Company 类 POI + 名称相等 + **城市过滤（25km 容差）**
- 失败 → NotFound（§110——不随意选同城另一家公司）

### 坐标/地图点击（§107）
- snap 最近可路由边（300m）

## 二、关键缺陷发现与修复

**BLOCKER 级**：access_node 节点在路网中**可能是孤立节点**（POI 门前连接点无路网边）——
被 P2-02 节点压缩（只保留被边引用的节点）剔除——`node_index` 查不到 →
POI 解析全部失败（london NotFound）。**修复**：DestinationResolver 构造时
持有**全量节点位置索引**（uid→pos，来自 RoutingGraph.nodes——未压缩），
snap 到最近可路由边——**修复后 london 等全部解析成功**。

## 三、验证

### 单元测试（4 个，总计 28 全绿）
- POI 经 access_node 解析 + snap 正确
- Job 城市过滤（Company 唯一化）
- NotFound / Ambiguous 语义
- 坐标解析

### Europe v4 真实数据（7,918 POI）
```
[POI] london @ (-39547,-11584) → access_snap edge=471815 off=158m lateral=31m
[Job] eurogoodies@london → edge=62720（城市过滤生效——之前 Ambiguous）
[Coordinate] (-58456,32832) → edge=116318
```

## 四、门

- fmt / clippy 0 / 49 测试全绿（workspace 总数）
- 新增依赖：rusqlite（bundled，nav-dataset 内）

## 五、下一步

- P2-16 Signal linker：movement.SemaphoreId ↔ runtime signal（§111-117）
- P2-17 Navigation session：全模块协调状态机（§118-121）
