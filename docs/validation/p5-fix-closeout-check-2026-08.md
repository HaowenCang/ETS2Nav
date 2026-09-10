# P5 图缺陷修复收尾核对记录（2026-08-12）

**目标**：P5 图缺陷修复的收尾——逐项核对交付物、重跑全部门验证、归档 tag 并推送、回写进展文档。

**对应提交**：`7900bbc`（修复 + 关门报告 + 计划状态回写）；**tag**：`v0.6.0-p4p5p6`（附注 tag，`^{}` → `7900bbc`）。
**上游记录**：`p5-graph-defects-2026-08.md`（根因与排除检验）、`p5-graph-defects-verification-2026-08.md`（验证记录）。

---

## 一、本次会话工作清单

| 序 | 工作 | 产物 |
|---|---|---|
| 1 | 接手核对：读 PLAN.md §1/§3、PLAN-P3plus.md、git log/tag，确认实际进展与文档状态表的落差 | — |
| 2 | 缺陷 ②（ferry 端点）根因定位 | `tools/ferry-diag/` 诊断输出 |
| 3 | 编译器修复：端点改为 linked prefab 路网接入节点 | `map-compiler/src/ScsMapModel/SemanticMapBuilder.cs` |
| 4 | 验收指标落盘（diagnostics 字段） | `map-compiler/src/ScsMapModel/DatasetWriter.cs`、`tools/map-inspector/.../Program.cs` |
| 5 | 缺陷 ③ 口径修正 + 残余五路排除检验 | `nav-core/tools/od-corpus/src/main.rs`、`src/diag/od_components.rs` |
| 6 | 跨实现 oracle（TruckLib 数据级比对） | `tools/oracle-conn/OracleConn/` |
| 7 | `od_features` 候选选择口径修正（连带发现） | `nav-core/tools/od-corpus/src/main.rs` |
| 8 | 回归套件可复现性修复（2 处） | `run-p1-tests.bat`、`run-p2-tests.bat` |
| 9 | Europe v5 数据集重建 + 基线重生成 | `data/europe-v5/`、`od-baseline-europe-v5.txt` |
| 10 | 阶段关门报告（P4/P5/P6）+ 进展文档回写 | `docs/validation/p4-closeout-*.md`、`p5-closeout-*.md`、`p6-closeout-*.md`、`PLAN.md`、`PLAN-P3plus.md`、`README.md` |

## 二、交付物核对

| # | 交付物 | 状态 | 证据 |
|---|---|---|---|
| 1 | 编译器修复 | ✅ | `SemanticMapBuilder.ResolveTerminalNodes`（`roadTouched` 前移传入 `BuildFerries`）；诊断计数 `TerminalCount`/`TerminalDegraded`/`RoadsSkippedMissingNode`/`RoadsSkippedRail` |
| 2 | 诊断字段 | ✅ | `diagnostics.json`：`ferries.terminals=87`、`terminals_degraded=0`、`transit_nodes=122`、`transit_nodes_isolated=0`、`roads.skipped_missing_node=0`、`roads.skipped_rail=11088` |
| 3 | 分量诊断工具 | ✅ | `nav-core/tools/od-corpus/src/diag/od_components.rs`，`Cargo.toml` 以 `[[bin]]` 显式声明路径（**不放 `src/bin/`——`.gitignore` 的 `bin/` 规则会将其忽略而无法入库，已实测确认**） |
| 4 | 码头诊断工具 | ✅ | `tools/ferry-diag/FerryDiag/`（`--linkcheck` / `--prefab-conn` / `--dump-raw` / `--mvcomplete`） |
| 5 | 跨实现 oracle | ✅ | `tools/oracle-conn/OracleConn/`（TruckLib 数据级转储；对 6 sector 比对 diff = 0） |
| 6 | 指标修正 | ✅ | `od-check` 锚点池改活跃节点 + `OD-CHECK-COMPONENTS` / `OD-CHECK-MAIN` 输出行；`od_features` 取最短候选 |
| 7 | 数据集 | ✅ | `data/europe-v5/`（routing.graph 224,253,090 B / junction.graph 93,982,821 B / map.db 54,730,752 B / search.db 671,744 B） |
| 8 | 基准 | ✅ | `od-baseline-europe-v5.txt`（36 对 / 9 区域，missing=0） |
| 9 | 回归套件更新 | ✅ | 四套件 `DATASET` → europe-v5；基线路径 → v5；p1/p2 可复现性修复 |
| 10 | 关门报告 | ✅ | p4 / p5 / p6 closeout 三份 + p5 根因报告 + p5 验证记录 |
| 11 | 进展文档回写 | ✅ | `PLAN.md` §1/§4/§4.2/§5、`PLAN-P3plus.md` §5、`README.md`；`p5-od-corpus-2026-08.md` §六 历史遗留段加状态更新注 |
| 12 | 决策记录 | ✅ | `docs/decisions/ADR-008-ferry-terminal-endpoints.md`（Ferry/Train 端点语义） |

## 三、门验证实跑记录（本次收尾重跑）

| 门 | 结果 | 输出证据 |
|---|---|---|
| `cargo fmt --check` | **PASS** | exit 0 |
| `cargo clippy --all-targets` | **0 warnings** | 无 `^warning: ` / `^error` 匹配（新增诊断工具亦为零告警，未用 `#[allow]` 压制） |
| `cargo test` | **97 passed / 0 failed** | 各 test result 行全部 ok，无 FAILED / error[E |
| `run-p1-tests.bat` | **ALL PASS** | [1/6] unit 65 测试全绿；[1b/6] map-inspector 显式构建 PASS（本次新增）；[2/6] Berlin gate fatal 0 / 核心网 OD 477/500（95.4%）；[3/6] Germany gate fatal 0 / OD 482/500（96.4%）；[4/6] determinism `routing b81b21c82acf0bef` / `junction 2f3ae3e40a6f583d` 两构建一致；[5/6] Rust reader PASS；[6/6] Europe scale `failed_prefabs: 0` |
| `run-p2-tests.bat` | **ALL PASS** | [0/7] TRACE GEN PASS（本次新增自生成步）；[1/7] P1 ALL PASS；[2/7] FMT/CLIPPY/CARGO TEST PASS；[3/7] DATASET SMOKE PASS；[4/7] ROUTE REGRESSION PASS；[5/7] MATCH REPLAY PASS；[6/7] SIGNAL LINK PASS；[7/7] PERF SMOKE PASS |
| `run-p3-tests.bat` | **ALL PASS** | 链内 P1 ALL PASS → P2 ALL PASS → P3 ALL PASS；[3/4] SPEED LOOKAHEAD PASS（breaks=2）；[4/4] CAMERA VERDICT PASS（`VERDICT=NO-GO`） |
| `run-p5-tests.bat` | **ALL PASS** | [1/4] CARGO TEST / ROUTER TEST PASS；[2/4] FMT / CLIPPY PASS；[2.5/4] BUILD PASS；[3/4] **BASELINE GEN PASS**（删除基线后端到端重生成）+ OD REGRESS PASS；[4/4] OD CHECK PASS |

**退出码复核**：以 `cmd /c "run-xx.bat > log 2>&1 & echo BAT_EXIT=%ERRORLEVEL%"` 实测，`run-p3-tests.bat` 与 `run-p5-tests.bat` 均为 **BAT_EXIT=0**。经 pwsh 包装的作业状态曾显示 `exit code: 1`，系 PowerShell 将子进程 stderr 包装为 `NativeCommandError` 所致，与套件结果无关。

**基线确定性**：删除 `od-baseline-europe-v5.txt` 后由套件重新生成，SHA-256 与删除前一致（`C32BAD5F2C407C445F26892F84D5B191910ACBA51759FC289FE6ACBD8E718FA9`）。

## 四、关键指标（Europe v4 → v5）

| 指标 | v4 | v5 |
|---|---|---|
| transit 端点孤立 | 258 / 258 | **0 / 522** |
| 主分量节点数 | 273,410 | **286,214** |
| 主分量 / 活跃节点 | 75.78% | **79.35%** |
| transit 边（ferry + train） | 129（127+2） | 261（229+32） |
| 码头降级 | — | **0**（87 码头） |
| od-check uturns | 1 | **0** |
| od-check jumps | 0 | 0 |
| 可达率（活跃节点配对） | 61.05%（旧口径，不可比） | 61.20%（上界 62.96%） |

## 五、Tag 与推送记录（2026-08-12）

```
$ git log --oneline -1
7900bbc P5 图缺陷根因排查与修复 + P4/P5/P6 关门

$ git tag -l | tail
v0.3.0-p2
v0.4.0-p3
v0.5.0
v0.6.0-p4p5p6

$ git push origin main
   d4d8161..7900bbc  main -> main

$ git push origin v0.6.0-p4p5p6
 * [new tag]         v0.6.0-p4p5p6 -> v0.6.0-p4p5p6

$ git ls-remote --heads --tags origin
7900bbc98fa5c230d5ef06a8e02df5c2b8024184  refs/heads/main
248159b3fd61fa286495650a96cc7877951bcd97  refs/tags/v0.6.0-p4p5p6
7900bbc98fa5c230d5ef06a8e02df5c2b8024184  refs/tags/v0.6.0-p4p5p6^{}

$ git status --porcelain      # 空
$ git log origin/main..HEAD   # 空
```

本地工作区干净、`main` 与 `origin/main` 同步、附注 tag 与其解引用对象均已推送。

> **后续推进（2026-08-12 ~ 2026-09-11）**：本节记录的是 P5 修复收尾当时的推送状态。此后 `main` 依次推进 `92cff89`（本记录 + tag/提交号回写）→ `0928e47`（ADR-008）→ `4b5e945` → `00f9a54`（离线加固四项）→ `19eceb1`（数据集 Release 分发）→ `c5fe3d7`/`b0f8a05`/`a2acc57`/`8ea6fc4`（文档同步与自洽修正）→ `bdbd7ea`（B 侧采集链缺陷修复）→ `dafabc0`（回写与同步）；并新增数据发布 tag `dataset-europe-v5`。截至 2026-09-11（本条所记的最后一个提交为 `dafabc0`），本地与 `origin/main` 一致。最新同步与门验证复核见 `p4-p6-hardening-2026-08.md` §9。

## 六、已知限制（本次修复相关，登记不阻塞）

1. **残余断簇（源数据拓扑）**：74,503 个活跃节点分属 12,970 个非主分量；其中 67,410 节点（90.5%，11,184 分量）无任何跨分量 prefab，且道路/prefab 数据层与 TruckLib oracle 零差异——判定为 ETS2 源数据本身的拓扑断开。未修复（超出编译器职责）。
2. **1,786 个分量（7,093 节点）存在跨分量 prefab**，但其 movement 连通图自身不连通（`spanning_with_mv_disconnected=2763`），结合恢复完备性检验（`under_recovered=0`）判定非路由级连接件。
3. **掉头缺陷不可复核**：原登记边（395371→395373）属 v4 索引，v5 因节点压缩与边集合变化已重排，无法回溯定位；验收口径为「v5 上 od-check uturns=0」。
4. **oracle 覆盖范围**：数据级比对 6 个 sector（3,632 道路 / 1,015 prefab）；未做全欧洲比对（成本）与 PPD 导航语义层比对（TruckLib 本版本未暴露 PPD 导航模型）。
5. **数据集不入库**：`.gitignore` 含 `data/`；换机器需按 p5-closeout-2026-08.md §6 重建（约 4 分钟）。
6. **B 侧实机测试全部延后**（D6）：B1~B6 未执行；相关实机类限制见各阶段 closeout 报告。

## 七、结论

本次修复的交付物、门验证、tag 与推送均已完成并核对；四套回归套件与 cargo 门全绿，基线生成确定，工作区与远程同步。P4/P5/P6 三阶段按「机器可验证完成即关门 + 实机项登记已知限制」口径关门。

**项目剩余工作仅为 B 侧实机测试（B1~B6）。**
