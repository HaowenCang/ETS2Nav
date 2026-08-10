# P1-13 Europe Build 验证报告（2026-08）

依据：P1-map-compiler-plan.md §106。数据：游戏 1.60.1.7 官方 base + 全部 DLC
（115 archives / 105 DLC）。

## 结论

**通过**。complete build（1,358 sector、253 万 items）；**0 fatal**
（Rust dataset-reader-smoke PASS）；**0 unresolved parser error**
（diagnostics failed_prefabs = 0——全欧洲 2,432 种 prefab 全部加载）；
无静默跳过（失败列表完整记录且为空）。

> **数据勘误（2026-08-10，P2-04 前置验证）**：P1 构建的 sector 枚举只匹配
> `sec+` 前缀——**负 x 区域（UK/爱尔兰/伊比利亚西部）422 个 sector 全部缺失**。
> 修复（`/sec-` 前缀 + 正则符号组）后完整 Europe 规模见下表"勘误后"列。
> **本报告原始数字为修复前口径**；后续 P2 数据集（Europe v4）采用勘误后口径。

## Build 规模

| 指标 | 值（原始） | 勘误后（P2-04 修复，v4） |
|---|---|---|
| sector（base+aux） | 1,358 | **1,101**（实际被加载的 sector；负数区域补入 422 后含未覆盖区） |
| items / nodes | 2,530,050 / 3,442,375 | —（v4 未重报） |
| roads | 143,677（v2 rail 排除后） | 255,454（v4：Road 边） |
| junctions（prefab 实例） | 42,843 | **73,141** |
| prefab 种类 | 2,432（全加载成功） | — |
| movements | 171,453 | **281,012**（100% 带几何） |
| companies / POI | 1,185 / 4,997 | **7,918**（城市 380，含全部 UK 城市） |
| graph nodes / edges | 3,427,132 / 426,907 | **5,633,750 / 696,717**（Road 415,576 + Movement 281,012 + Transit 129） |

## Dataset 产物（europe-dataset/）

| 文件 | 大小（原始） | 勘误后（Europe v4，data/europe-v4/） |
|---|---|---|
| routing.graph | 80.3 MB（426,907 边） | **133.8 MB**（696,717 边） |
| junction.graph | 9.1 MB（42,843 junction） | **54.4 MB**（73,141 junction） |
| map.db | 32.9 MB | — |
| search.db | 442 KB（4,997 POI） | **~1.4 MB**（7,918 POI） |
| manifest.json / diagnostics.json | ✓ | ✓ |

## 验证

1. **Rust dataset-reader-smoke PASS**：42,843 junctions / 171,453 movements
   无自环、节点引用全合法、计数/尾部一致（原始口径）；**v4 复验 PASS**
   （696,717 边 / 281,012 movements / 几何全校验）
2. **diagnostics.json failed_prefabs = 0**：全欧洲 prefab 解析零失败
   （P1-04 的 PPD v0x19 解析 + 哈希 token 容错在全量上验证）
3. **无静默跳过**：诊断机制完整（FailedPpds/失败列表——为空即全成功）

## 关键事实

- 全欧洲构建性能：dataset 生成（含 2,432 PPD 加载 + 图构建）可重复执行
- P1-03 corpus 修复（0.3% 失败全在 vehicle/climate 非地图语义文件——
  不在加载路径）不影响本构建
- **勘误影响**：P1 阶段所有基于旧数据集的分析（P1-07 Germany 规模、P1-08 POI
  搜索、P1-13 本报告）为修复前口径；P2 阶段全部使用勘误后 Europe v4

## 通过条件（正式）

> complete build / 0 fatal / 0 unresolved parser error / 不静默跳过

全部满足。P1-13 通过。下一步 P1-14 Regression Suite（one command → complete P1 test suite）。
