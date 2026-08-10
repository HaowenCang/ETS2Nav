# P1-11 Dataset Writer 验证报告（2026-08）

依据：P1-map-compiler-plan.md §104 / ADR-004。数据：游戏 1.60.1.7。

## 结论

**通过**。Dataset 6 文件全部生成，**Rust dataset-reader-smoke 独立读取 PASS**
（无 C# runtime 依赖）——完成条件达成。

## 产物（--dataset <outdir>）

| 文件 | 格式 | Berlin | Germany |
|---|---|---|---|
| routing.graph | 紧凑二进制（magic/version/endianness/offset） | 1.36 MB / 6207 边 | 6.15 MB / 29585 边 |
| junction.graph | 紧凑二进制 | 133 KB / 693 junction | 631 KB / 3323 junction |
| map.db | SQLite（roads/junctions/movements 表） | 544 KB | 2.4 MB |
| search.db | SQLite FTS5（poi 全文索引） | 32 KB / 79 POI | 57 KB / 375 POI |
| manifest.json | JSON（版本/统计/文件清单） | ✓ | ✓ |
| diagnostics.json | JSON（构建诊断/失败列表） | ✓ | ✓ |

## routing.graph 格式（v1）

```text
magic "ETS2RG1"(7) + version u32 + endianness u32(0x12345678)
+ node_count u32 + edge_count u32
nodes[]: uid u64 + x/y/z i32（fixed 1/256）                    (20 B)
edges[]: from u32 + to u32 + kind u8 + length f32 + source u64
         + semaphore_id i32 + flags u8 [+ movement_id i32]    (26/30 B)
```

## junction.graph 格式（v1）

```text
magic "ETS2JG1"(7) + version u32 + endianness u32 + junction_count u32
junctions[]: uid u64 + prefab_token(64B) + node_count u8 + node_uids[]
             + movement_count u32
movements[]: entry u64 + exit u64 + length f32 + turn i8
             + semaphore_id i32 + signal_group_type_len u8 + type
```

## Rust reader-smoke（tools/dataset-reader-smoke）

零依赖 Rust 二进制，独立读取并校验：
- magic/version/endianness 校验
- 边节点引用越界检查（全部合法）
- junction movement 自环检查（0）
- 计数/尾部偏移一致（routing 6207 边、junction 693/3323、movements 2506/11512）

**PASS（Berlin 与 Germany 均通过）**。

## 设计决策

1. 并行边合法（多车道/多路径 movement 同 entry/exit——真实语义）
2. junction.graph 过滤自环 movement（无导航语义——数据集干净）
3. map.db/search.db 用 SQLite（ADR-004）；routing/junction.graph 用自定义紧凑
   二进制（无 .NET 序列化依赖——Rust 可读）

## 完成条件

> Dataset 可脱离 C# runtime 被独立读取。

✓ Rust reader-smoke 读取全部关键结构并校验通过。

## 通过条件（正式）

P1-11 通过。下一步 P1-12 Vector Tiles（map.pmtiles——graph-debugger/MapLibre 可直接加载）。
