# P2-07 Snap Model 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §54-57（SnapPoint/起点 Snap/目的地 Snap/Virtual Start-Goal）。
**产物**：nav-router crate（snap 模块）+ nav-graph 公共几何 API + CLI `snap` 命令。

## 一、实现

### SnapPoint（§54）
```rust
SnapPoint { edge_id: u32, offset: f64, position: (f64,f64,f64), tangent: f64, lateral: f64 }
```

### 起点 Snap（§55）
- `SnapPoint::from_match`：直接使用 MapMatch 的 edge+offset（**不重新 nearest-node**——避免起点吸附到错误道路）

### 目的地 Snap（§56）
- `snap_nearest(graph, spatial, x, z, radius)`：spatial 半径查询 → 逐边投影 → **最近可路由边**（Road/Movement；Ferry/Train 不吸附）
- POI 走 access_node（P1 已实现，P2-15 接入）

### Virtual Start/Goal（§57）
- `VirtualEndpoint { edge_id, offset, allow_forward, allow_backward }`
- `VirtualEndpoint::start(snap, allow_backward)` / `::goal(snap)`——route search 的虚拟段输入（P2-09 搜索接入），**不修改全局图**

### 共享重构
- `nav_graph::project_point`（matcher/snap 共用，消除重复实现）+ `nav_graph::polyline_length`

## 二、验证

### 单元测试（3 个）
- snap_nearest 吸附最近边（600m 处点 → 横边 offset 600）
- 半径外返回 None
- VirtualEndpoint 方向语义（start 允许 backward / goal 仅 forward；forward 剩余段计算）

### Europe 真实数据（data/europe-v4，357MB 稳定路径）
- `snap -58456,32832`（Berlin 中心）：300m 内 **112 候选边**，最近 **28.7m**（edge 116318）——城区密度正常
- 交叉核对：P2-06 匹配边 116316/116317 在 hits 内（lateral 59.9m）——spatial 查询无漏边
- 诊断中发现的 python 侧字段偏移误解已排除（Rust loader 布局正确，dataset info PASS）

## 三、门

- cargo fmt --check / clippy 0 warnings / **17 测试全绿**（matcher 3 + snap 3 + loader 3 + csr 2 + telemetry 4 + spatial 2）
- P1 Regression 不受影响（仅新增 crate/API）

## 四、下一步

- P2-08 Cost model（EdgeCostProvider + Fastest/Shortest/Balanced，§58-69）
- P2-09 Dijkstra Oracle + A*（虚拟起终点接入，§70-78）
