# Dataset 格式笔记（P1-11 ADR-004；P2-01 v2 升级）

来源：P1-11 Dataset Writer（2026-08）；**v2（P2-01，ETS2NAV_DATASET_VERSION=2）**。
格式：routing.graph/junction.graph 紧凑二进制（magic/version/endianness + 顺序固定布局——无 offset
table，读取端以计数+尾部偏移校验）；map.db/search.db SQLite；manifest/diagnostics JSON。
**v1 产物（format_version=1）不再被读取端接受**（版本策略：support version == expected）。

## 文件清单

| 文件 | 格式 | 内容 |
|---|---|---|
| routing.graph | 紧凑二进制 "ETS2RG1" | 节点 + 边（含 kind/movement_id/semaphore） |
| junction.graph | 紧凑二进制 "ETS2JG1" | junction + movements（含信号灯组类型） |
| map.db | SQLite | roads/junctions/movements 表 |
| search.db | SQLite FTS5 | poi 表 + poi_fts 全文索引 |
| manifest.json | JSON | 版本/统计/文件清单 |
| diagnostics.json | JSON | 构建诊断（failed_prefabs 等） |

## routing.graph 布局（v2）

```
magic "ETS2RG1"(7B) + version u32(2) + endianness u32(0x12345678)
+ node_count u32 + edge_count u32 + geom_point_count u32
nodes[]: uid u64 + x/y/z i32（fixed 1/256 定点）                  (20 B)
edges[]: from u32 + to u32 + kind u8 + length f32 + source_uid u64
         + geom_offset u32 + geom_count u16 + speed_limit i16 + road_class u8
         + semaphore_id i32 + flags u8                            (35 B)
         + [movement_id i32 当 flags bit3]                        (39 B)
geometry[]: x/y/z i32 × geom_point_count                          (12 B/点)
flags bit0 NoAi / bit1 GpsAvoid / bit2 Secret / bit3 有 movement_id
geom_offset 单位 = i32 坐标值索引（= 点数 × 3）；geom_count 0 = 无几何（读取端跳过）
speed_limit 三态：-1 未知 / 0 无限速 / >0 km/h
road_class：0=unknown 1=local 2=expressway 3=motorway（自 speed_class 映射）
kind：0=Road 1=JunctionMovement 2=Ferry 3=Train 4=ServiceAccess
```

## junction.graph 布局（v2）

```
magic "ETS2JG1"(7B) + version u32(2) + endianness u32 + junction_count u32
junctions[]: uid u64 + prefab_token(64B 定长 NUL 填充) + node_count u8
             + node_uids(u64 × node_count) + movement_count u32
movements[]: id u32 + entry u64 + exit u64 + length f32 + turn i8
             + semaphore_id i32 + group_type_len u8 + group_type(ASCII)
             + geom_offset u32 + geom_count u16                   (36 B + type)
geometry[]: x/y/z i32 × 总点数（movement 记录引用；geom_offset 单位同 routing.graph）
id = SemanticMap 原始 movement 索引（v2 显式化——消除自环过滤导致的隐式索引错位）
```

## 读取方契约（Rust dataset-reader-smoke）

- magic 7 字节（非 8——与 C# 写入一致）
- 自环 movement 在写入前过滤（entry==exit 无导航语义）
- 并行边合法（多车道/多路径同 entry/exit——不同 SemaphoreId 需保留）
- 坐标 fixed 1/256（i32 定点——±8.4e6 m 范围）

## 变更风险

C# 写入端与 Rust 读取端是两处独立实现——修改任一格式字段必须同步两端
（P1 收官评审建议：新增格式字段时 Rust smoke 先失败再通过）。
