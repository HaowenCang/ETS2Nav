# ADR-008 — Ferry/Train terminal endpoint semantics

**状态**：Accepted（2026-08-12，P5 图缺陷修复）
**依据**：P5 图缺陷排查；vendor/ref/wiki/Ferry.md；P5 audit（a3-correctness / a3-data-integrity）

## 背景

`FerryItem` 含两个标识节点的字段：

- `node_uid`——地图规范（Ferry.md）定义为「UID of the node of this item」，即该 ferry item **自身的标记节点**；
- `prefab_link_uid`——「UID of the prefab this ferry is linked to」（编辑器在 ferry item 位于 prefab 内且该 prefab 启用 ferry entrance 标志时建立链接）。

P1-04 至 P2-01 的实现（`SemanticMapBuilder.BuildFerries`）以 `node_uid` 作为航线端点。P5 审计发现该选择使全部 129 条 transit 边的端点落在 2–9 节点微型分量中；实测 `node_uid` 节点出边 0、入边 0（完全孤立），故 ferry 边不桥接任何路网。英国全境 4,438 节点分量因此无法经海峡轮渡接入欧陆主网。

## 决策

Ferry/Train 航线的端点取 **linked prefab 中与路网相连的节点**（road 端点 ∪ 有 movement 的 junction 节点），而非 `FerryItem.NodeUid`：

- 端点解析：`PrefabLinkUid != 0` 且该 prefab 在本图内时，取其 `NodeUids` 中属于 `roadTouched` 的节点集（去重）；
- 降级路径：无 prefab 链接、链接未解析、或解析结果为空时，回退 `FerryItem.NodeUid` 并置 degraded 计数（`terminals_degraded`），使降级可见而非静默；
- 同港口多码头节点的全连接沿用 `RoutingGraphBuilder` 既有行为（本次未改动其语义，仅端点集合变得正确）。

**验收判据**：端点必须至少有一条非 transit 边（即必须落在陆地路网上）；`diagnostics.json` 的 `transit_nodes_isolated` 计数器直接检验该条件。

## 后果

- 正向：transit 端点孤立数 258/258 → 0/522；主分量 273,410 → 286,214（75.78% → 79.35%）；transit 边 129 → 261（端点集合正确后全连接生效）；87 个码头全部解析到路网接入（`terminals_degraded=0`）。
- 正向：`transit_nodes_isolated` 与 `terminals_degraded` 作为诊断字段落盘，该缺陷类别若复现可被直接检出而非依赖分量统计推断。
- 反向：端点语义依赖 `prefab_link_uid` 的数据质量；未链接的 ferry item 将降级为标记节点端点（已计数，不静默）。
- 边界：该修复不改变残余断簇（74,503 活跃节点）；后者经五路独立检验判定为 ETS2 源数据拓扑，与本决策无关。

## 关联

`docs/validation/p5-graph-defects-2026-08.md`（根因与排除检验）、`p5-graph-defects-verification-2026-08.md`（验证记录）；ADR-007（POI access model，同类语义问题：区分 visual position 与 routing access node）。
