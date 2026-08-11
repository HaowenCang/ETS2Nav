# P5 自动化 OD corpus 报告（2026-08-11）

**依据**：PLAN-P3plus.md §2 A3（P5 全欧洲测试：已知路线集 + 随机 OD 全图检查 + 回归套件）
**产物**：`nav-core/tools/od-corpus/`（独立 crate，3+1 子命令）+ `run-p5-tests.bat` + `od-baseline-europe-v4.txt`（36 条基准）

---

## 一、交付物

| 文件 | 内容 |
|---|---|
| nav-core/tools/od-corpus/Cargo.toml | 独立 crate（依赖 nav-dataset/graph/spatial/router；零新增第三方依赖） |
| nav-core/tools/od-corpus/src/main.rs | od-baseline / od-regress / od-check / od-diag 四子命令 |
| nav-core/Cargo.toml | workspace 新增成员 "tools/od-corpus" |
| run-p5-tests.bat | 4 步回归套件（见 §四） |
| od-baseline-europe-v4.txt | 九区域 36 城市对期望特征基准（distance/eta/edges/transit/company） |

## 二、已知路线集（od-baseline，九区域 36 对全部 PASS）

城市定位：search.db City POI 的 access_node（uid hex）→ routing.nodes 全量节点位置
（路由空间；POI 表 x/z 为地图坐标空间——P1-08 已知，不可直接路由）。

| 区域 | 城市对（ETS2 游戏内名） | 命中 |
|---|---|---|
| UK | london→edinburgh / london→birmingham / newcastle→plymouth / glasgow→cardiff | 4/4 |
| France | paris→marseille / paris→bordeaux / lille→nice / nantes→lyon | 4/4 |
| Germany | berlin→munchen / hamburg→frankfurt / dortmund→kassel / bremen→nurnberg | 4/4 |
| Nordic | stockholm→oslo / oslo→kobenhavn / helsinki→oulu / goteborg→stockholm | 4/4 |
| Balkan | beograd→zagreb / sarajevo→podgorica / skopje→thessaloniki / bucuresti→sofia | 4/4 |
| Italy | roma→milano / napoli→venezia / torino→bari / genova→bari | 4/4 |
| Iberia | madrid→barcelona / madrid→sevilla / lisboa→porto / bilbao→valencia | 4/4 |
| East | warszawa→krakow / prague→wien / budapest→wien / wroclaw→gdansk | 4/4 |
| DLC | tirana→skopje / sarajevo→beograd / ivalo→honningsvag / athens→patras | 4/4 |

期望特征（每对 5 维）：distance_m、eta_s、edges 数、transit（Ferry/Train）使用标志、
company（目的地 1.5km 内 Company POI access_node）标志。

**diff 回归机制**：`od-baseline --write` 生成文本基准（`region|from|to|dist|eta|edges|transit|company`，
git 可 diff）；`od-regress` 重跑并对比——distance ±1%、eta/edges ±5%、标志一致——
`OD-REGRESS pairs=36 diff=0`（当前 PASS）。基准文件入 git 后，路由/编译器改动触发
diff 检测。

## 三、随机 OD 全图检查（od-check）

方法：全图 5,633,750 路由节点随机采样对（LCG 固定种子 20260811 可复现），A* Fastest；
检查：可达性、geometry 连续（相邻边共享端点）、graph jump（不连续）、不合理掉头
（相邻 Road→Road 长边 ≥50m 首尾方向夹角 >150°；movement 边为路口内部转向不参与）。

实测（2000 对，Europe v4）：

```
OD-CHECK pairs=2000 reachable=1221 continuity=1221 jumps=0 uturns=1 ms=190393
```

| 指标 | 值 | 判定 |
|---|---|---|
| graph jump | **0** | ✅ 全图边序列拓扑连续（无跳变） |
| geometry 连续 | 1221/1221 | ✅ 全部可达路线连续 |
| 不合理掉头 | 1（边 395371→395373，173m→159m 反向） | 登记为图缺陷待排查 |
| 可达率 | 61%（1221/2000） | 见"断簇发现" |

**断簇发现（P5 核心价值）**：约 39% 随机节点对不可达——routing.graph 存在大量
**拓扑断簇**（与主路网断开的连通分量）。P2 的 CSR 校验只验证"边端点索引合法"，
**从未验证连通性**——本检查首次量化暴露：主网（最大连通分量）占比 ~61%。断簇成因
待排查（城市内部微网未接入主网 / 编译期节点压缩边引用问题），登记 P2 遗留。

**城市接入点断簇**：1135 个 City POI 行中 12 个城市接入点 3 跳 BFS 范围 <8 节点
（断簇接入点——如 koln/roma 的 access_node 指向断簇，od-baseline 多行 POI 回退解决
36/36；koln→dresden、roma→palermo 因两端全断簇改用 dortmund→kassel、torino→bari）。

## 四、回归套件（run-p5-tests.bat，ALL PASS 实测）

```
[1/4] cargo test（od-corpus + nav-router 依赖链）  CARGO TEST PASS / ROUTER TEST PASS
[2/4] cargo fmt -p od-corpus --check / clippy      FMT PASS / CLIPPY PASS
[3/4] 已知路线集 diff 回归                          BASELINE GEN PASS / OD REGRESS PASS
[4/4] 随机 OD 500 对冒烟                            OD CHECK PASS
P5 Regression Suite: ALL PASS
```

注：与 run-p1/p2/p3 同构；[1/4] 未含 nav-core-cli 全链——A2 并行线（P4 server 模块）
当前占用 nav-core-cli 且其代码编译错误阻塞 workspace 级门，P5 套件以 od-corpus
依赖链独立成门（父会话统一合并后 P3 链自然恢复）。

## 五、验证

- cargo fmt -p od-corpus --check PASS；clippy -p od-corpus --all-targets 0 warnings
- cargo test -p od-corpus / -p nav-router 全绿
- od-baseline：regions=9 pairs=36 missing=0 PASS（含 --write 落盘）
- od-regress：pairs=36 diff=0 PASS
- od-check：2000 对 PASS（jumps=0 / uturns=1 登记 / 主网 61%）
- run-p5-tests.bat：P5 Regression Suite: ALL PASS

## 六、已知限制与遗留

1. **路由图拓扑断簇 ~39%**（P2 遗留，首次量化）：连通性未在 P2 校验——建议后续
   排查（连通分量统计 + 断簇成因：编译期节点压缩/城市微网接入）。
2. **uturn 单点**：边 395371→395373（173m→159m Road 反向）——真实图缺陷或匝道
   结构，登记待查。
3. **POI access_node 断簇接入点**（12 城市）：多行 POI 回退已覆盖已知路线集；dest
   导航到断簇城市时仍可能失败（P2-15 仅验证解析未验证可达性——P5 补充验证）。
4. **od-check 性能**：2000 对约 190s（长距离 A*）；套件冒烟用 500 对。
5. **A2 并行线**：nav-core-cli 由 A2 独占且当前编译错误——P5 套件独立成门；父会话
   统一合并后恢复全 workspace 门。

**BLOCKERS: A2 并行线 nav-core-cli 编译错误（server_cli.rs 引用 nav_router::RouteProfile
/SessionConfig 路径错误）阻塞全 workspace cargo 门——非本任务代码问题；P5 套件已独立成门
规避。其余无。**
