# P2-05 Spatial Index 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §42-44（Edge bbox 索引/查询/自适应半径）。
**产物**：nav-spatial crate（统一网格空间索引）+ nav-graph 几何保留 + CLI 基准。

## 一、实现

### 统一网格（cell 哈希）替代 R-tree
- **理由**：静态数据集上网格 = R-tree 的等价查询语义（覆盖 cell 集合 = 节点范围遍历），
  构建 O(E)、查询 O(cells+候选)，确定性、零依赖、实现 1/10
- **ponytail 标注**：查询 p99 超 10ms 或内存超标时升级 R-tree/STR 打包（P2-19 性能包）
- cell 尺寸 256m（默认）；每边按 polyline bbox 插入覆盖 cells
- 索引对象：Road + JunctionMovement（**Ferry/Train 不入索引**——计划 §42）
- `query_radius(x, z, r)`：覆盖 cells → bbox 最近点距离 ≤ r 粗判 → 候选 CSR 边 id
  （精确点到 polyline 距离由匹配器计算——计划 §43 语义）

### nav-graph 配合
- CompactGraph 增加 `edges_geometry` 点池字段（spatial 需要边几何）
- `edge_geometry(&self, e)` 简化签名

## 二、性能（Europe v4：696,717 边）

| 指标 | 实测 | 目标（§140） |
|---|---|---|
| 索引构建 | **47.5 ms** | — |
| 查询（Berlin 城区 500 次 r=100m） | **2.0 ms 总（4µs/次）** | p99 < 10 ms |
| 候选（城区） | 6549 / 500 查询 | — |
| cells | 65,696 | — |

## 三、验证

- **单元测试 2 个**（总计 12 个全绿）：半径查询命中/排除远边、Ferry 不入索引
- cargo fmt / clippy（0 warnings）/ test 全绿
- Europe v4 实测基准 PASS

## 四、下一步

- P2-06 Map Matching：消费 TelemetrySnapshot + CSR + spatial——
  候选评分（距离×航向×拓扑连续性×route bias，计划 §47）+ rolling candidate tracker（§49）
