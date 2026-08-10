# P1-13 Europe Build 验证报告（2026-08）

依据：P1-map-compiler-plan.md §106。数据：游戏 1.60.1.7 官方 base + 全部 DLC
（115 archives / 105 DLC）。

## 结论

**通过**。complete build（1358 sector、253 万 items）；**0 fatal**
（Rust dataset-reader-smoke PASS）；**0 unresolved parser error**
（diagnostics failed_prefabs = 0——全欧洲 2,432 种 prefab 全部加载）；
无静默跳过（失败列表完整记录且为空）。

## Build 规模

| 指标 | 值 |
|---|---|
| sector（base+aux） | 1,358 |
| items / nodes | 2,530,050 / 3,442,375 |
| roads | 143,677（v2 rail 排除后；v1 149,814） |
| junctions（prefab 实例） | 42,843 |
| prefab 种类 | 2,432（全加载成功） |
| movements | 171,453 |
| companies / POI | 1,185 / 4,997 |
| graph nodes / edges | 3,427,132 / 426,907（v2 rail 排除后；v1 435,392） |

## Dataset 产物（europe-dataset/）

| 文件 | 大小 |
|---|---|
| routing.graph | 80.3 MB / 80,328,057 B（426,907 边，v2 实测） |
| junction.graph | 9.1 MB（42,843 junction） |
| map.db | 32.9 MB / 32,890,880 B（v2 实测） |
| search.db | 442 KB（4,997 POI） |
| manifest.json / diagnostics.json | ✓ |

## 验证

1. **Rust dataset-reader-smoke PASS**：42,843 junctions / 171,453 movements
   无自环、节点引用全合法、计数/尾部一致
2. **diagnostics.json failed_prefabs = 0**：全欧洲 prefab 解析零失败
   （P1-04 的 PPD v0x19 解析 + 哈希 token 容错在全量上验证）
3. **无静默跳过**：诊断机制完整（FailedPpds/失败列表——为空即全成功）

## 关键事实

- 全欧洲构建性能：dataset 生成（含 2,432 PPD 加载 + 图构建）可重复执行
- P1-03 corpus 修复（0.3% 失败全在 vehicle/climate 非地图语义文件——
  不在加载路径）不影响本构建

## 通过条件（正式）

> complete build / 0 fatal / 0 unresolved parser error / 不静默跳过

全部满足。P1-13 通过。下一步 P1-14 Regression Suite（one command → complete P1 test suite）。
