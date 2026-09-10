# P5 关门报告（2026-08-12）

**阶段**：P5 全欧洲测试（v0.2 §18、§66；PLAN-P3plus.md §2 A3）
**关门口径**：沿用 P2/P3 先例——机器可验证部分完成即关门（tag），实机项登记已知限制（D6 决策：实机测试延后）。
**tag**：`v0.5.0`（A2~A5 交付，2026-08-12）+ 本报告随 P5 图缺陷修复提交。

---

## §1 阶段目标与出口条件

PLAN.md §4：P5 = 「全欧洲官方地图测试（automated OD corpus，v0.2 §18、§66）」，入口条件 P1/P4 产物，出口验收「数千 OD 自动检查通过」。

PLAN-P3plus.md §2 A3 拆分三项：
1. 已知路线集（九区域，含 origin/destination/expected features）；
2. 随机 OD 检查（数千对：可达性、geometry 连续、不合理掉头、graph jump）；
3. 与 run-p1/p2 同构的单命令套件。

## §2 出口条件对照

| # | 出口条件 | 状态 | 证据 |
|---|---|---|---|
| 1 | 已知路线集覆盖九区域且可 diff 回归 | ✅ | `od-baseline-europe-v5.txt` 36 对 / 9 区域（UK/France/Germany/Nordic/Balkan/Italy/Iberia/East/DLC），`od-baseline` missing=0；`od-regress` pairs=36 diff=0 |
| 2 | 随机 OD 数千对自动检查 | ✅（口径内） | `od-check` 2000 对（Europe v4）与 500 对冒烟（套件步骤）；jumps=0 / uturns=0（修复后）/ 可达率 61.2%（活跃节点配对口径，理论上界 62.96%） |
| 3 | geometry 连续 | ✅ | `continuity` = `reachable`（全部可达路线相邻边共享端点）；jumps=0 |
| 4 | 单命令套件 | ✅ | `run-p5-tests.bat` 4 步 ALL PASS（BAT_EXIT=0） |
| 5 | 全欧洲口径 | ✅ | 数据集 `data/europe-v5`：2,202 sector / 115 archives / 105 DLC / 5,633,750 节点 / 696,849 边 |
| 6 | 图缺陷处置（本阶段核心增量） | ✅ | `p5-graph-defects-2026-08.md` + `p5-graph-defects-verification-2026-08.md` |

## §3 本阶段交付物

| 项 | 位置 |
|---|---|
| OD corpus 工具 | `nav-core/tools/od-corpus/`（`od-baseline` / `od-regress` / `od-check` / `od-diag`）+ `src/bin/od-components.rs`（分量诊断，7 种模式） |
| 回归套件 | `run-p5-tests.bat` |
| 基准 | `od-baseline-europe-v5.txt`（36 对；SHA-256 `C32BAD5F…8FA9`，生成确定） |
| 数据集（修复代） | `data/europe-v5/` |
| 图缺陷报告 | `docs/validation/p5-graph-defects-2026-08.md`（根因/修复/排除检验） |
| 验证记录 | `docs/validation/p5-graph-defects-verification-2026-08.md`（套件实跑/指标对照/oracle 比对） |
| 原 P5 报告 | `docs/validation/p5-od-corpus-2026-08.md`（A3 交付，2026-08-11） |
| 诊断工具 | `tools/ferry-diag/`（码头接入诊断）、`tools/oracle-conn/`（TruckLib 数据级 oracle） |

## §4 性能与规模

| 项 | 值 |
|---|---|
| od-check 2000 对耗时 | 约 190 s（长距离 A*；套件冒烟用 500 对约 37 s） |
| Europe 全量构建 | 约 4 分钟（2,202 sector） |
| 数据集体积 | routing.graph 224,253,090 B / junction.graph 93,982,821 B / map.db 54,730,752 B / search.db 671,744 B |
| 构建确定性 | 同 sector 集两次构建 running routing.graph / junction.graph SHA-256 一致（P1 [4/6] PASS） |

## §5 已知限制（登记）

1. **残余断簇（源数据拓扑）**：74,503 个活跃节点分属 12,970 个非主分量；其中 67,410 节点（90.5%，11,184 分量）经证明无任何跨分量 prefab，且道路/prefab 数据层与 TruckLib oracle 零差异——判定为 ETS2 源数据本身的拓扑断开（公司场院、停车区、独立路网片段等）。未修复：修复需人工判定地图数据意图，超出编译器职责。精确口径见 p5-graph-defects §3.3。
2. **1,786 个分量（7,093 节点）存在跨分量 prefab**，但其 movement 连通图自身不连通（`spanning_with_mv_disconnected=2763`），结合恢复完备性检验（`under_recovered=0`）判定非路由级连接件；登记备后续按需复核。
3. **掉头缺陷不可复核**：v4 检出的具体边（395371→395373）在 v5 索引重排后无法回溯定位；以「v5 上 uturns=0」为验收口径。
4. **TruckLib oracle 覆盖范围**：数据级比对 6 个 sector（3,632 道路 / 1,015 prefab）；未做全欧洲比对（成本）与 PPD 导航语义层比对（TruckLib 本版本未暴露 PPD 导航模型）。
5. **随 P5 一并发现的套件可复现性缺陷已修复**：`run-p2-tests.bat` 对 `%TEMP%\real.navtrace` 的隐式依赖（新增 [0/7] 自生成步）；`run-p1-tests.bat` 依赖未纳入解决方案的 Debug 二进制（新增 [1b/6] 显式构建步）。二者均为验证完整性缺陷，非产品缺陷。
   **同类第三项（2026-09-11 补记）**：`run-p2`/`run-p3`/`run-p5` 的每步结论写成 `if %FAIL%==1`（累积标志）而非该步 `errorlevel`，致**第一步失败后后续每步均报 FAIL（即使其命令成功）**——一个 clippy 错误会被放大成六项失败。已改为逐步独立判定，`FAIL` 仅用于退出码。该缺陷由一次误判排查暴露，详见 `p4-p6-hardening-2026-08.md` §9.2。
6. **od-baseline 口径修正**：候选选择由「首个可行」改为「最短可行」（原因与物理合理性核对见 p5-graph-defects §5）。原有 v4 基准 27/36 对数值随之下调，`od-baseline-europe-v4.txt` 保留作历史对照。
7. **实机项（B 侧，D6 延后）**：P5 无实机依赖，本阶段为 PLAN-P3plus 中唯一可自足出口的阶段。

## §6 验证命令

前置：数据集不入库（`.gitignore` 含 `data/`），需先在本地构建规范数据集。

```
cd E:\Projects\Pi\ETS2Nav
:: 前置——构建 Europe v5 数据集（约 4 分钟）
tools\map-inspector\MapInspector\bin\Release\net9.0\map-inspector.exe ^
    --install "E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2" ^
    --all-sectors --dataset data\europe-v5

run-p5-tests.bat                 # P5 Regression Suite: ALL PASS
run-p3-tests.bat                 # 链内 P1 -> P2 -> P3 全部 ALL PASS
cd nav-core && cargo fmt --check && cargo clippy --all-targets && cargo test
```

## §7 结论

P5 出口条件全部满足：九区域已知路线集（36 对）可 diff 回归、全欧洲随机 OD 检查通过（jump 0 / uturn 0 / 可达性逼近理论上界）、单命令套件 ALL PASS。A3 交付时登记的图缺陷已完成根因排查与修复（UK 孤立与 ferry 悬空为真实编译器缺陷，已修复；残余断簇经五路独立检验判定为源数据拓扑，已登记）。

**P5 关门。**
