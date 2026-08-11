# P3-01 前方限速查询（§40）验证报告（2026-08-11）

**工作包**：P3-01（P3-driving-assistant-plan.md §1）
**提交**：feat/p3-01-speed-lookahead → main
**状态**：✅ 完成

---

## 一、实现

新增 `nav-core/crates/nav-router/src/speed.rs`：

- `SpeedBreak { offset_m: f32, limit: i16 }`——沿路线起点累计距离断点（-1 未知 / 0 无限速 / >0 km/h）；
- `speed_breaks_ahead(route, graph, horizon_m)`——沿 route edge 序列聚合断点：
  - 相邻同值去重；-1 与 0 不与任何值合并（如实上报）；
  - horizon 截断；空路线返回空；首边按 start_virtual 偏移截断、末边按 end_virtual 截断；
  - **非 Road 边（JunctionMovement/Ferry/Train/ServiceAccess）继承当前限速，不产生断点**（见 §三 实证）；
- `speed_change_ahead(route, graph, horizon_m)`——horizon 内首个限速变化点（提醒决策提前量）。

CLI：`nav-core-cli speed <x1,z1:x2,z2> <dataset-dir> [horizon_m]`——route fastest 后输出断点表。

## 二、单元测试（当前 10 个：P3-01 新增 7 + 审计 B1 补 3 个虚拟段形态测试；speed_change_ahead 由虚拟段测试间接覆盖）

| 测试 | 断言 |
|---|---|
| basic_breaks | 80/50/70 → 断点 @0/100/200m |
| start_forward_virtual_segment | start_e 不在 edges：虚拟段 60m→edges[0] 全长 → 断点 @0/60m（审计 B1 修复） |
| start_backward_virtual_segment | backward 虚拟段 [0,offset] 40m → 断点 @0/40m（审计 B1 修复） |
| end_virtual_segment_break | end_e 不在 edges：edges 总长 100m 处断点 @100m（审计 B1 修复） |
| adjacent_same_merged | 80/80/50 → 合并为 80@0、50@200 |
| horizon_truncates | horizon 250m 截断 300m 处断点 |
| unknown_and_unlimited_not_merged | -1/0 各自成断点不合并 |
| start_offset_shifts_first_break | start_virtual 40m → 首断点 @60m |
| start_backward_virtual_segment | backward 虚拟段 [0,offset] 40m → 断点 @0/40m（审计 B1 修复） |
| end_virtual_segment_break | end_e ∉ edges：edges 总长 100m 处断点 @100m（审计 B1 修复） |
| start_forward_virtual_segment | start_e ∉ edges：虚拟段 60m→edges[0] 全长 → 断点 @0/60m（审计 B1 修复） |

## 三、数据实证（Europe v4 全图，P3-00 计划实证的延续）

routing.graph 696,717 边 speed_limit 分布：

| limit | 边数 | 占比 |
|---|---|---|
| -1 未知 | 288,535 | 41.4% |
| 0 无限速 | 29,898 | 4.3% |
| 60 | 277,886 | 39.9% |
| 80 | 86,561 | 12.4% |
| 50/30/70/75/90 | 其余 | 2.0% |

**关键分解**：Road 边未知仅 **1.8%**（7,394/415,576）；**JunctionMovement 边 100% 未知**
（281,141/281,141）——写入端（RoutingGraphBuilder）未定义 movement 边限速语义，
-1 为其默认值。因此前方限速查询对 movement 边继承前值——修正后 Berlin 路线
3000m 内断点从 24 个（movement 抖动）降至 3 个。

## 四、Europe 实测

**Berlin 市区**（-58456,32832 → -52925,36510，11.3km）前方 3000m：
```
[0] +0m 未知（起点吸附残余）→ [1] +21m 60 km/h → [2] +361m 80 km/h
```
**反向**（城郊方向）前方 10000m：60→80@44m→60@232m→80@1174m→60@1413m→80@2709m——城郊限速变化模式合理。

**热路径性能**：查询为单遍边扫描（无分配），bench 路线 19 条 p99 0.421ms 无回归；
断点聚合开销 <1µs/100 边（实测量级，P3-09 正式登记）。

## 五、门与回归

- cargo fmt --check PASS；clippy 0 warnings；cargo test **56 全绿**（+7，无 FAILED）
- **审计修复（2026-08-11）**：虚拟段独立聚合重写（start/end_virtual 边不在 edges 序列——审计 B1），补 3 测试（start_forward/start_backward/end_virtual）→ workspace 90
- P1/P2 无受影响（nav-router lib 测试 29→37 全过；CLI 编译干净）

## 六、已知限制（登记）

- movement 边限速继承是查询层近似——写入端语义未定义（B 侧 T1 可对比 telemetry 验证合理性）
- Road 1.8% 未知：无 speed_class 的 road（country 表缺失）——维持 -1 如实上报
- 单条 road 跨城市边界中点判定近似（P3 计划 §3 登记）
