# P5 图缺陷根因排查与修复报告（2026-08-12）

**目标**：PLAN-P3plus.md §5 A3 遗留——P5 OD corpus 审计发现的 routing.graph 拓扑缺陷（A3c/A3d 审计登记）。本报告给出四项缺陷的根因定位、修复、量化指标与验证证据。

**触发**：p5-od-corpus-2026-08.md §六「已知限制与遗留」第 1~2 项，及 a3-correctness / a3-data-integrity 审计的补充发现（UK 孤立 + ferry 悬空）。

---

## §1 结论摘要

四项登记的图缺陷中，**第 ① ② 项为真实编译器缺陷并已修复**；第 ③ 项的量化口径本身存在方法缺陷（已修正），其残余部分经五路独立检验判定为**源数据拓扑**，非编译器成因；第 ④ 项在修复后于检查样本中不再复现。

| # | 登记缺陷 | 判定 | 修复后指标 |
|---|---|---|---|
| ① | 英国全境 4,438 节点独立分量，与欧陆主网断开 | **编译器缺陷（已修复）** | 该分量已并入主分量；主分量 273,410 → **286,214** |
| ② | 129 条 transit 边端点落 2–9 节点微型分量（轮渡未接入路网） | **编译器缺陷（已修复）** | transit 端点孤立数 **258 → 0**；transit 边 129 → 261（含 32 train） |
| ③ | 随机节点对不可达率约 39%（主 WCC 约 76%） | 口径缺陷（已修正）+ 残余为源数据 | 主分量占比 **75.78% → 79.35%**；活跃节点对可达率 61.2%，理论上界 62.96% |
| ④ | 掉头图缺陷（边 395371→395373，173 m→159 m 反向） | 检查样本未复现 | od-check uturns **1 → 0** |

---

## §2 缺陷 ② / ① 的根因与修复

### 2.1 根因（实证）

`map-compiler/src/ScsMapModel/SemanticMapBuilder.cs` 的 `BuildFerries` 以 `FerryItem.NodeUid` 作为航线端点。该节点按地图规范（vendor/ref/wiki/Ferry.md）只是「该 ferry item 自身的节点」，**不承载路网连接**；真正的路网接入点是 `FerryItem.PrefabLinkUid` 所指 prefab 的节点。

诊断输出（`tools/ferry-diag`，安装态）：

```
FERRY sector=sec-0008-0002 port=calais node=002935deecb08e89 pos=(-30888,-5464)
      inGraph=True anyEdge=False isLandNode=False
      prefabLink=396e8e4c3f510001 linkResolved=True linkToken=dlc_no_39
      linkNodes=2 linkLandNodes=1 nearestLand=45m
    FERRYNODE out=0 in=0
```

Calais 码头节点出边 0、入边 0（完全孤立），而其 linked prefab 的节点 `396e8e4c89d10002` 是**有边的陆地节点**，距码头 45 m。多佛侧同理（254 m，prefab `94uk`，3 节点中 2 个为陆地节点）。

后果：每条 ferry 边连接两个孤立标记节点，两端同处一个 2–7 节点微型分量，**不桥接任何路网**。英国全境路网（4,438 节点分量）因此无法经海峡轮渡接入欧陆主网。

### 2.2 修复

`BuildFerries` 端点解析改为 **linked prefab 中与路网相连的节点**（`roadTouched` = road 端点 ∪ 有 movement 的 junction 节点），并保留降级路径：

```csharp
private static IReadOnlyList<ulong> ResolveTerminalNodes(
    FerryItem f, Dictionary<ulong, SemanticJunction> junctionByUid,
    HashSet<ulong> roadTouched, out bool degraded)
{
    degraded = false;
    if (f.PrefabLinkUid != 0 && junctionByUid.TryGetValue(f.PrefabLinkUid, out var j))
    {
        var nodes = j.NodeUids.Where(roadTouched.Contains).Distinct().ToArray();
        if (nodes.Length > 0) return nodes;
    }
    degraded = true;
    return new[] { f.NodeUid };
}
```

`roadTouched` 原先在 `Build()` 中于公司 access 判定处构造，本次将其计算前移并传入 `BuildFerries`（同一集合，语义不变）。

### 2.3 验收指标（Europe v5 全量，2,202 sector / 115 archives / 105 DLC）

`diagnostics.json` 新增字段直接反映该指标：

| 指标 | 修复前（v4） | 修复后（v5） |
|---|---|---|
| `ferries.terminals` | — | 87 |
| `ferries.terminals_degraded` | — | **0** |
| `ferries.transit_nodes_isolated` | **258** | **0** |
| transit 边数 | 129（ferry 127 + train 2） | 261（ferry 229 + train 32） |
| 主分量节点数 | 273,410 | **286,214** |
| 主分量 / 活跃节点 | 75.78% | **79.35%** |
| 活跃分量数 | 13,015 | 12,971 |

transit 边数由 129 增至 261：端点解析到 prefab 节点后，同港口多码头节点的**全连接**（RoutingGraphBuilder 既有逻辑）开始生效；此前端点集合过小（每港 1 个孤立节点）使连接对稀少。

---

## §3 缺陷 ③ 的口径缺陷与五路排除检验

### 3.1 口径缺陷（原诊断方法错误）

原 `od-check` 的锚点池取 **routing.graph 全部节点**（Europe 5,633,750 个），其中 5,273,033 个（93.6%）**不被任何边引用**（item 节点）。这些点 `snap_nearest` 必然失败并被 `continue`，使「不可达率」实际只统计了落在稠密区的样本，且主分量占比从未被直接输出。原报告中的「主网 WCC 273,410/360,804 活跃节点 = 75.8%」另一处又用了活跃节点口径，两处口径混用。

**修正**：`od-check` 锚点池改为 `CompactGraph` 的活跃节点，并新增独立输出行：

```
OD-CHECK-COMPONENTS active_nodes=360717 components=12971 main_component=286214
                    main_ratio=0.7935 reachable_prob_upper_bound=0.6296
OD-CHECK pairs=500 reachable=306 reachable_ratio=0.6120 continuity=306 jumps=0 uturns=0
OD-CHECK-MAIN main_ratio=0.7935 reachable_ratio=0.6120 upper_bound=0.6296
```

可达率 61.2% 对理论上界 62.96%（Σfᵢ²）——已逼近上界，说明**可达性损失几乎全部来自分量划分本身，而非搜索失败**。

### 3.2 残余 74,503 个非主网活跃节点的五路排除

| # | 假设 | 检验方法 | 结果 |
|---|---|---|---|
| 1 | 道路被解析丢弃 | `RoadsSkippedMissingNode` 计数 | **0**（sector 集合完整，无端点缺失丢弃） |
| 2 | movement 未物化为边 | `--mvtest`：movement 簇 ⊆ 图分量恒成立，`mv_clusters < graph_comps` 即矛盾 | **contradictions=0**（69,424 个 junction） |
| 3 | ControlNode→NodeUid 映射错误 | `--scale`：几何弧长 / movement.Length = 锚定缩放 s，局部单位为米故正确时 s≈1 | **99.99% 落在 [0.5, 2]**（281,012 条） |
| 4 | movement 恢复算法漏恢复 | `--mvcomplete`：PPD 导航图连通划分应被 movement 连通划分细化或相等 | **under_recovered=0**（3,185 个 prefab token）；`depth_limited_ge16=0`（MaxDepth 上限未触发） |
| 5 | 数据层解析歧义/丢失 | **TruckLib 独立实现 oracle**（ADR-005）逐项 diff：road（uid + 两端节点）与 prefab（uid + 节点表） | **3,632 条道路 / 1,015 个 prefab 零差异** |

第 5 项为决定性证据：同一批 6 个 sector（sec-0002-0001/+0002、sec-0003-0001/+0002、sec-0004-0001/+0002）下，本项目解析器与 TruckLib 输出的 item 集合**完全一致**（`oracle-conn` 与 `ferry-diag --dump-raw` 逐行 diff，diff 条目 = 0）。单 sector 亦一致（559/559、866/866、116/116）。

### 3.3 残余归因量化

`--crossing`：对每个非主分量，判定是否存在「同时含该分量与主分量节点」的 prefab（潜在连接件）：

```
DIAG-CROSSING nonmain_comps=12970 with_crossing_prefab_comps=1786 with_nodes=7093
              without_crossing_prefab_comps=11184 without_nodes=67410
```

即：**11,184 个分量（67,410 节点，占残余 90.5%）不存在任何跨分量 prefab**。结合检验 1 与 5（道路数据零丢失、与 oracle 一致），这些分量的唯一可能连接件是道路，而道路集合已被证明完整——**故判定为 ETS2 源数据本身的拓扑断开**（公司场院/停车区/独立路网片段等在游戏地图数据中即未接入主网）。

剩余 1,786 个分量（7,093 节点，占残余 9.5%）存在跨分量 prefab。检验 2 显示全部 2,763 个跨分量 prefab 的 movement 连通图本身即不连通（`spanning_with_mv_disconnected=2763`），即这些 prefab 在其自身导航语义下也不连接两组节点。结合检验 4（恢复无漏），可判定其非路由级连接件。

**登记为已知限制（P2/P5 遗留，性质：源数据）**，附精确量化口径备后续复核。

---

## §4 缺陷 ④（掉头）

原登记：边 395371→395373（173 m→159 m，相邻 Road→Road 首尾方向夹角 >150°，od-check 检出 1 处）。

修复后 `od-check` 检查样本中 `uturns=0`（v5，500 对与 2,000 对均无检出）。原检出边号属 v4 数据集索引，v5 因节点压缩与边集合变化（transit 边 129→261）索引已重排，**不做逐边回溯比对**；以「当前数据集上 od-check 掉头检查无检出」为验收口径，原具体边是否为真缺陷无法在重排后重新定位，登记为不可复核项。

---

## §5 od-baseline 口径修正（连带发现）

修复过程中发现 `od_features` 的候选选择存在口径问题并一并修正：

- **原实现**：遍历「起点城市行 × 终点城市行」组合，返回**首个**可行路线。
- **问题**：某城市首行接入点由「不连通」变为「连通」时（本次修复的直接后果），基准值会因回退顺序改变而跳变，与图质量无关。实证：`newcastle→plymouth` 由 47,619 m 变为 89,924 m（含轮渡）——前者来自第 2 行接入点，后者来自第 1 行。
- **修正**：取全部可行组合中**距离最短**者，语义为「该城市的最佳道路接入」，且确定、可复现。

修正后基准（`od-baseline-europe-v5.txt`，36 对）较 v4 有 27 对距离缩短。以 ETS2 地图 1:19 比例换算核对物理合理性（内部米 × 19 ≈ 实际公里）：

| 城市对 | v5 内部距离 | ×19 折算 | 实际公路里程 |
|---|---|---|---|
| berlin→munchen | 29,811 m | 566 km | ≈580 km |
| paris→marseille | 43,652 m | 829 km | ≈780 km |
| hamburg→frankfurt | 27,518 m | 523 km | ≈500 km |

修正后数值与物理实际吻合度优于 v4（v4 berlin→munchen 33,082 m → 629 km），支持该口径修正为改进而非回归。

---

## §6 交付物

| 项 | 位置 | 说明 |
|---|---|---|
| 编译器修复 | `map-compiler/src/ScsMapModel/SemanticMapBuilder.cs` | `ResolveTerminalNodes` + `roadTouched` 前移；`RoadsSkippedMissingNode` / `RoadsSkippedRail` / `TerminalCount` / `TerminalDegraded` 诊断计数 |
| 诊断字段 | `map-compiler/src/ScsMapModel/DatasetWriter.cs` | `diagnostics.json`：`ferries.terminals/terminals_degraded/transit_nodes/transit_nodes_isolated`、`roads.skipped_missing_node/skipped_rail` |
| 分量诊断工具 | `nav-core/tools/od-corpus/src/bin/od-components.rs` | 模式：`--gap`（碎片空隙）/`--junk`→`--junc`（跨分量 prefab）/`--scale`（端点映射）/`--mvtest`（物化一致性）/`--jvcomplete`→`--mvcomplete` 由 ferry-diag 提供/`--crossing`（可归因性）/`--dump`/`--jinfo`/`--local` |
| 指标修正 | `nav-core/tools/od-corpus/src/main.rs` | `component_stats` + `OD-CHECK-COMPONENTS`/`OD-CHECK-MAIN` 输出行；锚点池改活跃节点；`od_features` 取最短候选 |
| 跨实现 oracle | `tools/oracle-conn/OracleConn/` | TruckLib 数据级 oracle（road/prefab/ferry 逐项转储） |
| 码头诊断工具 | `tools/ferry-diag/FerryDiag/` | `--linkcheck` / `--prefab-conn` / `--dump-raw` / `--mvcomplete` |
| 数据集 | `data/europe-v5/` | 修复后 Europe 全量（routing.graph 224,253,090 B；junction.graph 93,982,821 B） |
| 基准 | `od-baseline-europe-v5.txt` | 36 对，min-distance 口径 |
| 回归套件 | `run-p1/p2/p3/p5-tests.bat` | DATASET → europe-v5；基线 → v5；可复现性修复（见 §7） |

sector 集合为 2,202 个（含 `.aux`），与 P1-13 的「1,358 base + aux」口径不同：P1-13 报 679 sector 为 Europe build 的 base 口径统计，本次 `--all-sectors` 同时加载 `.base` 与 `.aux` 两类文件，故计数更大；items 4,183,948 / nodes 5,660,074 与 P1-13 的 1358 sector 口径一致可对照。

---

## §7 回归套件可复现性修复

排查中发现两处使套件**不可在干净机器上成立**的缺陷，一并修复：

1. **`run-p2-tests.bat` [5/7] / [7/7] 隐式依赖 `%TEMP%\real.navtrace`**——该文件既不在仓库中，也无生成步骤；文件缺失时 map-match 与 bench 步骤必然失败。修复：新增 [0/7] 步，缺失时以 `nav-core-cli syntrace -58456,32832:-52925,36510 <dataset> %TEMP%\real.navtrace` 生成；[5/7] 改用 `%TRACE%` 变量。
2. **`run-p1-tests.bat` 调用未纳入解决方案的 Debug 二进制**——`tools/map-inspector` 不在 `map-compiler/MapCompiler.sln` 中，`dotnet test` 不会重建它；套件直接调用 `bin/Debug/net9.0/map-inspector.exe`，在干净检出上会因二进制缺失而失败，或在陈旧二进制上**假通过**。修复：新增 [1b/6] 步显式 `dotnet build tools/map-inspector/MapInspector/MapInspector.csproj -c Debug`。

两项均为验证完整性缺陷（非产品缺陷），修复后方可认为套件结果可信。

---

## §8 验证记录

见 `docs/validation/p5-graph-defects-verification-2026-08.md`（套件实跑输出与指标复算）。

## §9 已知限制（登记）

1. **残余断簇（源数据）**：74,503 个活跃节点分属 12,970 个非主分量；其中 67,410 节点（90.5%）无跨分量 prefab，判定为源数据拓扑断开。未做修复——修复需人工判定游戏地图数据意图，超出编译器职责。
2. **掉头缺陷不可复核**：v4 检出的具体边号在 v5 索引重排后无法回溯（§4）。
3. **rail 排除**：11,088 条 `speed_class=rail` 道路按 P1 决策不入路由网（`roads.skipped_rail`）。不影响道路网连通性判定。
4. **UK 路网内部**：英国分量已并入主网，但英国境内仍存在若干小分量（与 §9.1 同源）。经轮渡接入后，跨海峡 OD 可达；UK 内部次网可达性取决于源数据。
5. **TruckLib oracle 范围**：数据级比对覆盖 6 个 sector（3,632 道路 / 1,015 prefab）；未做全欧洲 2,202 sector 比对（成本）与语义层（PPD 导航曲线）比对（TruckLib 本版本未暴露 PPD 导航模型）。
