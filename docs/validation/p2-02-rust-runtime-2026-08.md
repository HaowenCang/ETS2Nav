# P2-02 Rust Runtime 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §26–30（Dataset Loader/版本策略/manifest/§19-20 CSR）。
**产物**：nav-core/ Rust workspace（nav-dataset / nav-graph / nav-core-cli）。

## 一、交付

### nav-core workspace（计划 §10 结构）
```
nav-core/
├─ Cargo.toml                workspace（resolver 2）
├─ crates/
│  ├─ nav-dataset/           v2 loader（routing.graph/junction.graph/manifest）
│  ├─ nav-graph/             CSR 紧凑图 + 节点压缩 + 反向邻接
└─ tools/nav-core-cli/       dataset info 命令
```

### nav-dataset（§26-30）
- `load_dataset(dir)`：manifest（dataset_version==2 校验）→ routing.graph + junction.graph
- 版本策略（§28）：**support version == expected**——v1 或未知版本 → DatasetVersionMismatch 拒绝启动
- 边界校验（§27）：magic/version/endianness/计数/节点范围/几何范围/尾部偏移全部检查
- Edge 全字段：kind/length/source_uid/geometry/speed_limit(-1/0/>0)/road_class/semaphore/movement_id/flags

### nav-graph（§19-20）
- **节点压缩**：只保留被边引用的节点——Europe 3,427,132 → **215,557 活跃（6.3%）**
- CSR 出边（node_offsets/edge_ids/edges）+ 反向邻接（in_offsets/in_edge_ids）
- 边几何按范围引用（geom_start/geom_len）
- GraphStats：分类计数/speed 未知统计

### nav-core-cli（§122-123）
- `dataset info <dir>`：版本/加载时间/节点压缩率/边分类/几何/speed 未知/带灯 movement

## 二、性能（计划 §139 目标：数秒级加载）

| 数据集 | 加载 | 活跃节点 | 粗略内存（边+几何） |
|---|---|---|---|
| Berlin v2 | 0.00 s | 3,316 / 59,685（5.6%） | 1.8 MB |
| **Europe v2** | **0.18 s** | 215,557 / 3,427,132（6.3%） | 116.4 MB |

Europe 全图 0.18s 远超"数秒级"目标；内存远低于 500MB 上限（§145）。

## 三、验证

- **单元测试 5 个**：loader 手工 v2 字节流全链路（节点/边/几何/speed/class/movement_id）+
  版本不匹配拒绝 + 损坏截断拒绝 + CSR 压缩/连续性/反向邻接 + 非活跃节点剔除
- cargo fmt --check / cargo clippy（0 warnings）/ cargo test 全绿（计划 §159 PR 最低要求）
- 真实数据：Berlin/Germany/Europe v2 全部加载 PASS

## 四、调试记录

1. **头部字段偏移**：node_count 在 15..19（初稿误用 19..23）——Rust 无 C# 的编译期保护，格式常量必须与 C# 写端逐一核对
2. **CSR degrees 用原始索引**（应压缩索引）——节点压缩后索引空间变化的经典错误
3. **测试文件并行竞争**：3 个测试共用 `{pid}_routing.graph` 文件名互相覆盖——测试文件名必须含测试名

## 五、下一步

- P2-03 Telemetry/trace（nav-telemetry + trace recorder/replayer）
- P2-04 前置验证（UK 方向/限速实测/环岛 corpus）
- P2-05 Spatial index（edge bbox R-tree，匹配候选查询）
