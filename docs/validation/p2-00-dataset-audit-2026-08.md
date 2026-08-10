# P2-00 Dataset Consumer Audit（2026-08-10）

**依据**：P2-navigation-core-plan.md §11–25（P2-00 工作包）。
**目的**：判断 Dataset v1 是否满足 Navigation Core 各模块的消费侧需求；
冻结 P2 数据需求清单，作为 P2-01 Dataset v2 规格依据。
**方法**：对照实际代码（DatasetWriter.cs / RoutingGraphBuilder.cs / SemanticMap.cs /
map-inspector Program.cs / dataset-reader-smoke main.rs）+ P1 实产物。

---

## 1. Dataset v1 实际内容（代码验证）

### routing.graph v1（ETS2RG1）

```text
magic(7) + version u32 + endianness u32 + node_count u32 + edge_count u32
nodes:  uid u64 + x/y/z i32（fixed 1/256，±8.4e6 m，分辨率 ~4 mm）     (20 B)
edges:  from u32 + to u32 + kind u8 + length f32 + source_uid u64
        + semaphore_id i32 + flags u8 [+ movement_id i32]             (26/30 B)
flags:  bit0 NoAi / bit1 GpsAvoid / bit2 Secret / bit3 有 movement_id
```

### junction.graph v1（ETS2JG1）

```text
magic(7) + version u32 + endianness u32 + junction_count u32
junction: uid u64 + prefab_token(64B NUL 填充) + node_count u8
          + node_uids(u64 × count) + movement_count u32
movement: entry u64 + exit u64 + length f32 + turn i8
          + semaphore_id i32 + group_type_len u8 + group_type(ASCII)
```

### map.db（SQLite，非热路径）

```text
roads:    uid / node0 / node1 / look / speed_class / speed_limit / direction / length
junctions / movements 表（含 SignalGroupType）
search.db: poi + poi_fts（FTS5）
```

---

## 2. 逐模块审计

| 消费模块 | 所需字段 | v1 状态 | 差距 |
|---|---|---|---|
| **Map Matching** | 车辆↔edge 距离 | nodes 坐标 ✓（端点） | **edge polyline 缺失**——长弯/匝道/环岛系统性误差（§13） |
| | heading vs edge tangent | — | 端点方向 ≠ 投影点 tangent（§48） |
| | 拓扑连续性 | from/to ✓ | — |
| | spatial 候选查询 | 无索引（v1 无） | 需 runtime 自建（P2-05） |
| **Route Cost** | length | ✓ | — |
| | speed_limit | **✗ 不在 routing.graph**（map.db roads 表有） | 热路径不可查 SQLite（§21）→ v2 需 hot metadata |
| | road_class/speed_class | **✗**（map.db 有 speed_class） | v2 需 hot |
| | 信号灯统计 delay | semaphore_id ✓ | 只需 id≥0 判断，够用 |
| | GpsAvoid/Secret/NoAi | flags ✓ | — |
| **Route Geometry** | 完整 polyline | **✗** | v2 geometry table（§15-18） |
| **Maneuver** | movement TurnType | turn i8 ✓（几何近似） | v2 几何细化（§97）需 CurvePath |
| | junction topology | junction.graph node_uids ✓ | — |
| **Roundabout** | movement 拓扑（entry/exit u64） | ✓ 可推导 | 几何细化需 CurvePath |
| **Transit** | Ferry/Train edges | **✗ RoutingGraphBuilder 只加 Road+JunctionMovement**（grep 证实零 Ferry/Train 分支） | v2 必须补齐（§23-24） |
| **Signal Link** | PPD SemaphoreId | ✓（edges + movements） | — |
| | signal head 位置/方向 | **✗**（PPD 有灯位置，未序列化） | v2 静态 signal geometry（§113） |
| **回溯/关联** | source_uid | ✓ | — |
| | movement_id 关联 | edge.MovementId → junction.graph 隐式索引 | **脆弱点**：junction.graph 过滤自环后隐式索引 vs SemanticMap 原始索引可能错位（当前 0 自环无实际影响；v2 建议显式 movement_id） |

---

## 3. 审计结论

### 已满足（Dijkstra/A* 原型可直接消费）

- from/to/kind/length/source_uid/semaphore_id/flags/movement_id 齐全
- 坐标精度（1/256 ≈ 4 mm）远超匹配需求
- 并行边合法保留（多车道/多灯）
- turn i8 可支撑 V1 maneuver 粗分类（Δ<30° 直行 / >150° U / 左负右正）

### 必须补（Dataset v2 变更清单——冻结为 P2-01 规格）

| # | v2 变更 | 理由（计划条目） | 阻塞模块 |
|---|---|---|---|
| V2-1 | **edge polyline**（geometry table，Point3 i32 1/256 复用） | §13-16 匹配距离/tangent | Map Matching |
| V2-2 | **movement polyline**（CurvePath → 世界坐标） | §18/§97 转向细化、环岛 | Maneuver/Roundabout |
| V2-3 | **speed_limit hot**（-1/0/数值三态） | §21-22 热路径禁 SQLite | Route Cost |
| V2-4 | **road_class/speed_class hot** | §21 时间 profile 速度模型 | Route Cost |
| V2-5 | **Ferry/Train edges**（origin/dest terminal + travel_time/distance/penalty） | §23-24 跨海路线 | Transit |
| V2-6 | **signal head 位置/方向** | §113 runtime association | Signal Link |
| V2-7 | **显式 movement_id**（junction.graph） | 消除隐式索引脆弱点 | 数据一致性 |
| V2-8 | geometry 采用**自适应曲线采样**（弦差控制，非固定间距） | §17 城市弯道精确/直路不冗余 | 全部几何消费方 |

### 不阻塞（runtime 自建，不改 schema）

- spatial index（R-tree，P2-05 在 Rust 侧构建）
- node compaction（CSR，P2-02 loader 阶段）

---

## 4. 数据需求冻结（P2-01 输入）

Dataset v2 概念模型（对齐计划 §15）：

```text
ETS2NAV_DATASET_VERSION = 2
RoutingEdgeV2:
  from / to / kind / length / source_uid
  geometry_offset / geometry_count      ← V2-1
  speed_limit_kph                       ← V2-3（-1 未知 / 0 无限速 / >0）
  road_class                            ← V2-4
  semaphore_id / movement_id / flags
```

geometry table：统一 Point3[]（x/y/z i32，1/256 定点），Road 与 JunctionMovement
同一 API；junction.graph 增加 movement 显式 id（V2-7）与 signal head 静态几何（V2-6）；
ferry/train 以 RoutingEdgeKind 进入 routing.graph（V2-5）。

版本策略：P1 tag v0.2.0-p1 不修改；Map Compiler main 向前演化产生 v2；
C# writer 与 Rust reader 同时修改（先让旧 reader test 失败——§160 双实现约束）。

---

## 5. 前置验证提醒（P2-04 独立工作包）

审计同时确认 P1 遗留项与 P2 的依赖关系：

- L1 LeftHandTraffic：UK/Ireland 方向验证（P2-04，Gate G2）——v2 重建前完成
- L2 Speed：speed-validator 实车闭环（P2-04，Gate G3）
- L3 Roundabout：专项 corpus（P2-04/P2-14，Gate G11）
- L4 TurnType 几何近似：V2-2 修复（P2-01）
- L5/L6 geometry：V2-1/V2-2
- L7 ferry/train：V2-5
- L8 speed metadata：V2-3/V2-4
- L9 runtime signal mapping：V2-6 + P2-16

全部 9 项 P1 遗留均已有明确 P2 归属。
