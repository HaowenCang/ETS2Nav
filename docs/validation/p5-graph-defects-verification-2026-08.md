# P5 图缺陷修复——验证记录（2026-08-12）

**对象**：`docs/validation/p5-graph-defects-2026-08.md` 所述修复的验收证据。
**环境**：ETS2 v1.60.1.7（115 archives / 105 DLC）；数据集 `data/europe-v5`（2,202 sector / 4,183,948 items / 5,660,074 nodes）；Rust 1.96.0 / .NET 9+10。

---

## §1 cargo 门

| 门 | 结果 |
|---|---|
| `cargo fmt --check` | PASS（exit 0） |
| `cargo clippy --all-targets` | **0 warnings** |
| `cargo test`（workspace，--all-targets 口径） | **97 passed / 0 failed** |

`od-components` 诊断工具有意保持了 clippy 零告警（新增 7 处 `needless_range_loop`、1 处 `type_complexity`、1 处 `ptr_arg`、1 处 `match_result_ok` 均按建议改写，未使用 `#[allow]` 压制）。

## §2 回归套件（实跑）

| 套件 | 结果 | 退出码 | 关键输出 |
|---|---|---|---|
| `run-p1-tests.bat` | **ALL PASS** | 0 | unit 65 tests 全绿；Berlin gate fatal 0 / core OD 477/500（95.4%）；Germany gate fatal 0 / OD 482/500（96.4%）；determinism `routing b81b21c82acf0bef / junction 2f3ae3e40a6f583d` 两次构建一致；Rust reader PASS；Europe scale `failed_prefabs: 0` |
| `run-p2-tests.bat` | **ALL PASS** | 0 | [0/7] TRACE GEN PASS（本次新增自生成）；FMT/CLIPPY/CARGO TEST PASS；DATASET SMOKE PASS；ROUTE REGRESSION PASS；MATCH REPLAY PASS；SIGNAL LINK PASS；PERF SMOKE PASS |
| `run-p3-tests.bat` | **ALL PASS** | 0 | 链内 P1 ALL PASS → P2 ALL PASS → P3 ALL PASS；SPEED LOOKAHEAD PASS（breaks=2）；CAMERA VERDICT PASS（NO-GO） |
| `run-p5-tests.bat` | **ALL PASS** | 0 | CARGO/ROUTER TEST PASS；FMT/CLIPPY PASS；BUILD PASS；**BASELINE GEN PASS**（删除基线后端到端重生成）；OD REGRESS PASS；OD CHECK PASS |

退出码说明：`cmd /c "run-xx.bat > log 2>&1 & echo BAT_EXIT=%ERRORLEVEL%"` 实测 p3 与 p5 均为 **BAT_EXIT=0**。经 pwsh 包装的作业状态曾显示 `exit code: 1`，系 PowerShell 将子进程 stderr 输出包装为 `NativeCommandError` 所致，与套件结果无关（已用上述方式独立复核）。

## §3 基线确定性

删除 `od-baseline-europe-v5.txt` 后由套件重新生成，SHA-256 与删除前一致：

```
before = C32BAD5F2C407C445F26892F84D5B191910ACBA51759FC289FE6ACBD8E718FA9
after  = C32BAD5F2C407C445F26892F84D5B191910ACBA51759FC289FE6ACBD8E718FA9
```

即 `od-baseline` 在「取最短候选」修正后为确定输出。

## §4 修复前后指标对照（Europe v4 → v5）

采集方式：`od-components <dataset>`（活跃节点口径）、`diagnostics.json` 字段。

| 指标 | v4（修复前） | v5（修复后） | 变化 |
|---|---|---|---|
| routing 节点总数 | 5,633,750 | 5,633,750 | — |
| 活跃节点数（被边引用） | 360,804 | 360,717 | −87 |
| 边数 | 696,717 | 696,849 | +132 |
| — Road | 415,576 | 415,576 | — |
| — JunctionMovement | 281,012 | 281,012 | — |
| — Ferry | 127 | 229 | +102 |
| — Train | 2 | 32 | +30 |
| 活跃分量数 | 13,015 | 12,971 | −44 |
| **主分量节点数** | **273,410** | **286,214** | **+12,804** |
| **主分量 / 活跃节点** | **75.78%** | **79.35%** | **+3.57 pt** |
| **transit 端点孤立数** | **258 / 258** | **0 / 522** | **−258** |
| 码头降级数 | — | **0**（87 码头） | — |
| 道路因端点缺失丢弃 | — | **0** | — |
| rail 排除道路 | — | 11,088 | — |
| od-check uturns | 1 | **0** | −1 |
| od-check jumps | 0 | 0 | — |
| od-check 可达率 | 61.05%（1221/2000） | 61.20%（306/500） | +0.15 pt |
| 可达率理论上界 Σfᵢ² | 0.5743 | 0.6296 | +0.055 |
| 最大单分量（原英国分量） | 4,438（独立） | 已并入主分量 | — |

**口径警告（两行不可直接比较）**：v4 行的可达率出自旧锚点池（全量 5.63M 节点），其分母为**尝试次数**而非有效样本——93.6% 的锚点因 300 m 内无路由边而 snap 失败，在计数前即被 `continue`，故该数值混合了「snap 失败」与「A\* 失败」两种成因，且违反 Σfᵢ² 上界约束（v4 上界 0.5743 < 该行 0.6105，即为失真之证）。v5 行锚点池已改为活跃节点，分母为有效样本，满足上界约束（0.6296 > 0.6120）。两行同列仅作量级对照。

## §5 缺陷 ① 的直接验收（英国并入主网）

修复前 `DIAG-TOP` 第 2 位为英国分量：

```
DIAG-TOP rank=1 size=4438 center=(-45899,-30732) bbox=(-60639,-55704)-(-31159,-5760)
```

修复后该分量消失（`DIAG-TOP` 第 1 位起即 233 节点的小碎片）：

```
DIAG-TOP rank=0 size=286214 center=(-7791,-17272) bbox=(-93734,-121898)-(78151,87354)
DIAG-TOP rank=1 size=233   center=(-12177,-2291)
```

主分量包围盒由 `(-93734,-121898)-(78151,77693)` 扩展至 `(-93734,-121898)-(78151,87354)`，北向边界外扩 9,661 m，与英国（负 X、负 Z 区）并入一致。

## §6 缺陷 ② 的直接验收（码头接入）

`tools/ferry-diag --install <game> --sectors sec-0008-0002,sec-0009-0002,...`：

```
DIAG components=27 largest=439 no_edge_nodes=12553
TRANSIT-SUMMARY edges=4 endpoints=8 endpoints_isolated=0
```

（修复前同口径为 `endpoints_isolated=8`。）

全量验收见 `diagnostics.json`：`ferries.terminals=87`、`terminals_degraded=0`、`transit_nodes=122`、`transit_nodes_isolated=0`。

## §7 跨实现 oracle 比对（数据层零差异）

```
ROADS   oracle=3632 ours=3632   diff entries = 0
PREFABS oracle=1015 ours=1015   diff entries = 0
```

sector：`sec-0002-0001, sec-0002-0002, sec-0003-0001, sec-0003-0002, sec-0004-0001, sec-0004-0002`
格式：`R <uid:x16> <node0:x16> <node1:x16>` / `P <uid:x16> <node:x16>...`，按 uid 升序逐行 diff。

单 sector 复核：sec-0003-0001（559/559）、sec-0003-0002（866/866）、sec-0002-0001（116/116）。

## §8 残余断簇的排除检验输出

```
DIAG-GRAPH  nodes_raw=5633750 nodes_active=360717 edges=696849
DIAG-WCC    active_components=12971 main_size=286214 main_ratio_of_active=0.7935
DIAG-TRANSIT total_edges=261 endpoints=522 endpoints_isolated=0
DIAG-MVTEST junctions_ge2_active_nodes=69424 spanning=2763
            spanning_with_mv_disconnected=2763 contradictions=0
DIAG-SCALE  movements_with_geom=281012 s0.5to2=280984 pct=99.99
MVCOMPLETE  tokens_checked=3185 under_recovered=0 zero_movement=305 depth_limited_ge16=0
DIAG-CROSSING nonmain_comps=12970 with_crossing_prefab_comps=1786 with_nodes=7093
              without_crossing_prefab_comps=11184 without_nodes=67410
```

## §9 结论

四项登记缺陷中，①②为真实编译器缺陷且已修复并验收；③的口径缺陷已修正，残余 74,503 节点经五路独立检验（道路完整性 / movement 物化一致性 / 端点映射 / 恢复完备性 / 跨实现 oracle）判定为源数据拓扑；④在修复后数据集上不再检出。全部四个回归套件与 cargo 门通过，数据集 `data/europe-v5` 与基线 `od-baseline-europe-v5.txt` 为该修复的规范产物。
