# P4/P6 离线加固验证记录（2026-08-12）

> **2026-09-11 补记（P4R Batch 2）**：本文中的 `verify-ui-chain.py` 已重命名为
> `verify-server-protocol.py`（全文引用同步）。该脚本不启动浏览器、不执行 app.js、
> 不触碰 DOM，准确定位是 **nav-server 协议集成测试**。§1 关于「remaining 递减」断言
> 的根因结论（syntrace heading）在当时成立，但 Batch 2 复跑发现该断言还有第二个
> 脆弱点——固定 60 帧观测窗口会落在 tracker 与 matcher 的启动对齐相位内，得到
> best_run=1 的假失败；现已改为「观测到证据为止 + 有界预算」，并为 wrap 判定补了
> 10 项确定性回归用例（`--selftest`）。详见 `docs/validation/p4r-batch2-2026-09.md`。

**范围**：审计阶段登记为「不强制修复」的遗留项中可离线处理者——UI 链验证不稳定、§9 mods 指纹漏报、§57 自动缩放/自动恢复、UI 事件类型分发；外加数据集外部分发与 B 侧会话准备。
**性质**：前四项中**三项经复核为真实缺陷（含两项规范违背）**，非风格问题；详见各节「性质」标注。

---

## §1 verify-server-protocol.py「remaining 递减」断言不稳定

**性质**：真实缺陷（合成 trace 与实时遥测的朝向约定不一致）。

### 根因

`nav-core-cli syntrace` 生成的点表已逐点计算 yaw，但写入帧时以**恒等四元数**填充 heading：

```rust
let (x, y, z, _yaw) = route_pts[idx];
... heading: [0.0, 0.0, 0.0, 1.0],
```

`nav_router::session` 以 `quat_yaw(snap.heading)` 还原航向后交给匹配器，而匹配器以
`w_heading * (1 - d_eff/heading_scale)` 参与打分。恒等四元数使合成帧恒为「yaw = 0」，
而路线实际朝向各异——巡航段候选边与假航向夹角超阈值，匹配持续失配，`remaining_m`
因此不单调递减。审计记录该断言 3 次复跑 2 PASS 1 FAIL。

### 修复

将 yaw↔quat 转换上移至 `nav-telemetry`（heading 字段的归属层），并消除 `nav-router`
中的重复私有实现：

| 项 | 位置 |
|---|---|
| `yaw_to_quat(yaw) -> [f32;4]` | `nav-core/crates/nav-telemetry/src/lib.rs`（新增，含约定说明） |
| `quat_yaw(q) -> f64` | 同文件（自 `nav-router/src/session.rs` 上移，原私有副本删除） |
| `IDENTITY_HEADING` 常量 | 同文件（标注「绕 Y 轴」约定） |
| syntrace 写入 | `nav-core-cli/src/main.rs`：`heading: nav_telemetry::yaw_to_quat(yaw)` |

约定：SCS 四元数为**绕 Y 轴**偏航，纯偏航四元数 `(0, sin(θ/2), 0, cos(θ/2))`，
`quat_yaw` 为其逆；yaw 采用 `atan2(dz, dx)`（与 matcher 的 `project_point` tangent 一致）。

### 验证

| 项 | 结果 |
|---|---|
| 新增单测（nav-telemetry） | `yaw_quat_round_trip`（7 个角度，误差 <1e-4）、`yaw_quat_is_unit_y_rotation`（x/z 分量为 0、模长 1、yaw=0 → 恒等）、`yaw_quat_axis_convention`（+X / +Z 前进交叉核对） |
| 匹配质量（同一条合成 trace，11111 帧） | **HIGH 10816 / MEDIUM 254 / LOW 41 / UNMATCHED 0**（HIGH 97.3%，横向距离均值 0.6 m） |
| 原失败断言复跑 | **8/8 PASS**（3 次 + 5 次两批；`remaining` 末值 11009 → 10892 → 10783 m 递减，回放循环正常） |

---

## §2 §9 mod 指纹漏报路径（P6 覆盖缺口）

**性质**：真实漏报（审计 a4-perf-truth M5 登记）。

### 缺口

原 `content_fingerprint` 仅覆盖游戏安装目录（版本 + archive 名/大小/UTC mtime + DLC 集合）。
mod 安装/更新**不改变安装目录任何元数据**，故数据集构建后启用或更新 mod 会使地图数据
改变而指纹仍报 MATCH——审计原文即称此为「真实漏报路径」。

### 复核：该机器存在大规模地图 mod

排查中发现本机装有 **ProMods 全量（11.2 GB，8 个文件）**，且其中
`promods-eu-map-v281.scs`（1.05 GB）**含 `/map` 条目**——即会改写本数据集所描述的地图。
`game.log.txt` 显示该次会话 `[mods] Active 17 mods (local: 0, workshop: 17)` 且
`promods-*.scs: Unmounted`，即当时 ProMods **未激活**——但「未激活」这一状态若不核对，
数据集与实际地图是否一致无从判断。这使原缺口从理论问题变为可复现的具体风险。

### 实现（`map-compiler/src/ScsResource/ModScanner.cs`）

| 机制 | 说明 |
|---|---|
| 来源枚举 | 本地 mod 目录（`Documents/ETS2/mod`，递归）与 Steam Workshop 内容目录（由安装目录推断 `<library>/steamapps/workshop/content/227300`） |
| **地图相关性探测** | 仅改写 `/map` 的 mod 会使数据集失效（涂装/内饰类不会）。探测＝读 archive 条目表查 `/map` 前缀，**不读内容** |
| 两种容器格式 | HashFS（`HashFsReader`）**与 zipfs（新增 `ZipfsProbe`，ZIP 中央目录枚举，含 ZIP64）** |
| 内容哈希分层 | 默认元数据（名/大小/UTC mtime，成本恒定）；`deep` 模式对**地图相关** mod 追加 SHA-256 |
| 模式标记 | 指纹串含 `mode=meta|deep`，避免「换算法」被误报为「数据变更」 |
| 不可探测显式化 | 两种格式均不识别的 archive 标为 `MapRelevant=null` 并计数，**不静默当 false** |

**zipfs 支持的必要性（实证）**：`game.log.txt` 记
`[zipfs] promods-eu-map-v281.scs: Created, 40594 entries`——ProMods 地图 mod 是 ZIP 容器。
仅实现 HashFS 探测时实测 `map_altering=0 unprobeable=57`，**恰好漏掉唯一真正的地图 mod**；
补 zipfs 后 `map_altering=1 unprobeable=1`。

### 验证（四路）

| 路径 | 命令 | 结果 |
|---|---|---|
| 正向（重建后一致） | `--check-fingerprint --install <game> --dataset data/europe-v5` | `FINGERPRINT MATCH`，exit 0 |
| **缺口闭合**（mod 变、安装不变） | 篡改副本 manifest 的 `mods_fingerprint` | `MODS-FINGERPRINT CHANGED stored=deadbeef… cur=123cb67d…`，exit 1（**旧实现此处会误报 MATCH**） |
| 安装变更（原路径未回归） | 篡改副本 `content_fingerprint` | `FINGERPRINT CHANGED`，exit 1 |
| 旧数据集（无 mod 字段） | 去除 manifest 的 mod 字段 | `MODS-FINGERPRINT ABSENT cur=…（该数据集构建于 mod 指纹引入前——重建后落盘）`，exit 1 |

确定性：`Scan(deep)` 两次独立运行得同一指纹（`123cb67d…cd099`，与重建时落盘值一致）。
成本：deep 模式对 1.05 GB 地图 mod 计内容哈希，端到端 **1.9 s**。

新增单测 15 项（`tests/ScsResource.Tests/ModScannerTests.cs`）：空目录确定性、新增 mod、
内容变化、zipfs 地图相关性、不可探测标注、deep 仅哈希地图相关、模式入哈希、workshop 纳入、
workshop 缺席、ZIP 条目枚举、前导斜杠兼容、非 ZIP 判定与异常、空 ZIP。ScsResource.Tests 由 8 → 23 全绿。

### 残留限制（登记）

`mods_fingerprint` 描述的是**磁盘上存在的 mod 集合**（变更检测），**不等于游戏实际激活集**。
ETS2 的激活集记录在存档/profile 内（本机 `profiles/` 为空、`steam_profiles/<hex>/` 仅含
config/controls，未定位到激活列表文件），本项目不解析存档。核对激活集的实际手段是
`game.log.txt` 的 `[mods] Active N mods` 行——本节的 ProMods 未激活判定即来自该行。

---

## §3 §57 自动缩放（规范违背）

**性质**：真实缺陷（与 v0.2 §57 明文相反）。

### 规范原文（v0.2 §57）

> 高速：显示较远范围。城市：显示附近道路。复杂路口：自动进入 Junction View。
> 用户手动拖动地图后：临时暂停 follow mode。**随后自动恢复。**

### 缺陷 1：速度项反相

原实现 `z = 13 + log2(max(10,d)) * -0.55 + log2(v/40) * 0.5`——速度项为**正**系数，
速度越高 zoom 越大（越放大），与「高速显示较远范围」相反。

修复：系数取负 `- Math.log2(v / 40) * 0.5`。核对：v=90 → log2(2.25)=1.17 → −0.585（拉远）；
v=30 → log2(0.75)=−0.415 → +0.207（拉近）。距离项与复杂路口项语义原本正确，未改动。

### 缺陷 2：自动恢复缺失

规范要求「随后自动恢复」，原实现仅提供 ◎ 按钮手动恢复（`btn-follow`），无自动路径。

修复：新增 `FOLLOW_RESUME_MS = 8000` 与 `FOLLOW_RESUME_MIN_KMH = 5`，在每帧快照中判定
「暂停 ≥8 s 且车速 ≥5 km/h」即自动恢复 follow；`pauseFollow`/`resumeFollow` 统一按钮与
事件处理（`dragstart`/`wheel` 绑定 `pauseFollow`）。

---

## §4 UI 事件类型分发

**性质**：真实缺陷（协议已约定的 `map_state` 事件从未被 UI 处理）。

### 缺陷

WS 接收端把**所有**帧无条件交给 `onSnapshot`，并以 `try/catch` 静默吞异常：

```javascript
try { onSnapshot(JSON.parse(ev.data)); } catch (e) { /* 忽略坏帧 */ }
```

`map_state` 帧不含 `state` 字段，`onSnapshot` 首行 `snap.state.toUpperCase()` 即抛错，
被 catch 吞掉——即 **A2a-M2 审计定稿的「设目的地后推送路线几何」契约在 UI 侧从未生效**。

### 修复

改为按 `type` 分发（`dispatchMessage`）：`vehicle` → `onSnapshot`；
`map_state` → `onMapState`（更新 `route-line` 几何）；未知类型计数并 `console.warn`。
解析失败仅丢弃该报文，处理异常改为 `console.error` 上报而非静默。

### 验证（协议侧 + 断言扩展）

扩展 `verify-server-protocol.py`：事件类型白名单断言（只允许 `vehicle`/`map_state`）+
「设目的地后必须收到含 polyline 的 map_state」。

实测：`seen=['map_state', 'vehicle']`，`map_state` polyline **1263 点**，UI CHAIN PASS。
复跑 4 次全 PASS。

排查记录：首轮该断言 FAIL（`pts=0`），根因是第 2 节读帧循环已消费 `map_state` 帧且不记录
类型——属测试脚本缺陷，非产品问题。修正为跨循环累计类型统计后通过。

---

## §5 回归验证（本次加固后）

| 门 | 结果 |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo clippy --all-targets` | 0 warnings |
| `cargo test` | **100 passed**（97 + 3 新增 yaw/quat 单测） |
| `dotnet test`（ScsResource.Tests） | **23 passed**（8 + 15 新增 mod/zip 单测） |
| `run-p1/p2/p3-tests.bat` | ALL PASS（`BAT_EXIT=0`；P1 链内 unit 65 + Berlin gate 95.4% + Germany gate 96.4% + determinism + Rust reader + Europe scale `failed_prefabs: 0`） |
| `run-p5-tests.bat` | ALL PASS（`BAT_EXIT=0`） |
| verify-server-protocol.py | 8/8 PASS（扩展后 4 项新增断言全过） |

## §6 数据集外部分发（Release 附件）

**动机**：数据集受 `.gitignore` 的 `data/` 规则约束不入库，克隆仓库后须本地重建（约 4 分钟）
**且需已安装 ETS2**——未拥有游戏者无法运行任何功能。发布为 Release 附件后，无需游戏即可
运行 Rust 导航核心、UI 联调、CI 与代码审查。

| 项 | 值 |
|---|---|
| Release | `dataset-europe-v5`（https://github.com/HaowenCang/ETS2Nav/releases/tag/dataset-europe-v5） |
| 附件 | `ets2nav-dataset-europe-v5.zip`，166.4 MB（原始 356 MB） |
| tag | `dataset-europe-v5`（数据发布用，不表示代码里程碑） |
| 内容 | routing.graph / junction.graph / map.db / search.db / manifest.json / diagnostics.json / README-dataset.txt |

**端到端验证**：

| 检查 | 结果 |
|---|---|
| 附件状态 | `gh release view` → `state=uploaded`，166.4 MB |
| 分发字节一致 | 下载件 SHA-256 `15B03A63…2831BB` == 本地归档 SHA-256（**MATCH**） |
| 归档可解压 | 7 个条目全部解出，文件名与大小与源一致 |
| 内容一致 | 解出 `routing.graph` SHA-256 `A20CE044…EBA9B8` == `data/europe-v5/routing.graph`（**MATCH**） |

**归档不入库**：`.gitignore` 新增 `ets2nav-dataset-*.zip` / `*.7z`（166 MB 二进制不进 git 历史）。

**指纹语义（在 Release notes 与归档内 README 中均明确标注）**：`content_fingerprint` 与
`mods_fingerprint` 是**构建机本地**的变更检测（游戏版本 / archive 名·大小·mtime / DLC 集合 /
mod 集合）。**在其他机器上检查必然报 CHANGED，这是预期行为而非数据集损坏**——指纹有效
需在目标机重建。该语义已在 §2 的实现中确立，此处仅确保分发时不产生误导。

---

## §7 B 侧会话准备

`docs/validation/b-session-runbook-2026-08.md`——单次会话（约 1 小时）采集 B1+B2+B3 的操作手册。

**前置条件已核实**：三个插件 DLL 已安装于 `bin\win_x64\plugins\` 且与仓库构建产物 **SHA-256 一致**
（B1 的复制步骤实际已完成）；`nav-core-cli` / `speed-validator` / `signal-lab` 均已构建。

**新增风险核对（本次排查发现，写入手册 §0.2）**：本机装有 **ProMods 全量 11.2 GB**，其中
`promods-eu-map-v281.scs`（1.05 GB）**含 `/map`**（即会改写地图）。最近一次游戏日志
（2026-09-02）显示 `[mods] Active 17 mods (local: 0, workshop: 17)` 且 `promods-*.scs: Unmounted`
——即当时 **ProMods 未激活**，数据集（base + DLC）与游戏内地图一致。但激活集可能已变，
故手册要求**会话前复核** `game.log.txt` 的 `[mods] Active` 行与 `local:` 计数。

同时发现 17 个激活 Workshop mod 中含两个灯态相关 mod，手册建议禁用其一：
`Flashing Green (Traffic Lights)`（绿灯闪烁行为——本项目最高风险功能为红绿灯 ±1 s 倒计时
与 GLOSA，该 mod 可能污染 T3 与 B3 判定）；`Different lenses of traffic lights`（灯罩外观，
可保留）。

---

## §8 关联文档更新

- `p4-ui-2026-08.md` §六：三项 MINOR 状态更新（不稳定断言 / §57 / 事件分发）
- `p4-closeout-2026-08.md` §5：已知限制第 6、8 项销账，第 7 项标注依赖 B5，新增第 10 项（合成 trace 终点偏移）
- `p6-performance-eval-2026-08.md` §2.3：§9 覆盖 4/9 → 6/9 + mods 实现与残留边界
- `p6-closeout-2026-08.md` §5：已知限制第 1 项更新（mods 覆盖闭合）+ 新增第 7 项（本机 ProMods 风险登记）+ §6 重建命令补 `--deep-mods`
- `PLAN.md` §1：当前状态补离线加固四项 + B 侧 runbook 指引
- `README.md`：当前阶段、关门报告入口、B 侧 runbook、数据集 Release 链接
- `.gitignore`：新增 `ets2nav-dataset-*.zip` / `*.7z`（发布归档不入库）
- `docs/validation/b-session-runbook-2026-08.md`（新建）

> 上表为本次加固提交（`00f9a54`）的文档变更。2026-09-11 的 GitHub 同步与其文档变更见 §9。

---

## §9 GitHub 同步记录（2026-09-11）

本项目采用「本地仓库为唯一写入源」（PLAN.md §5），故发布动作分两类：代码与文档经 `main` 推送，数据集经 Release 附件分发。本节记录两类发布的最终状态。

### 提交链（`d4d8161` → 发布代 `19eceb1`）

下表是**截至发布代 `19eceb1` 的时点快照，非穷尽列表**。其后的文档类提交（包括本节自身的回写与修正）不再逐条登记——理由与「`main` head 值」一节所述的自指问题相同：登记「记录该表的提交」会使该表立刻失效。追溯后续历史应用 `git log --oneline`。

| 提交 | 内容 |
|---|---|
| `7900bbc` | P5 图缺陷根因排查与修复 + P4/P5/P6 关门（tag `v0.6.0-p4p5p6`） |
| `92cff89` | P5 修复收尾核对记录 + 进展文档 tag/提交号回写 |
| `0928e47` | ADR-008：Ferry/Train 端点语义决策记录 |
| `4b5e945` | PLAN.md §5 补 `.gitignore` 注意事项（`bin/` 规则连带忽略 `src/bin/**`） |
| `00f9a54` | 离线加固四项（§1~§4） |
| `19eceb1` | 数据集 Release 分发记录 + README 分发入口（tag `dataset-europe-v5`）——**发布代** |

### Tag 与 Release

| 对象 | 指向 | 说明 |
|---|---|---|
| `v0.6.0-p4p5p6`（附注） | `7900bbc` | 代码里程碑：P4/P5/P6 关门 |
| `dataset-europe-v5`（轻量） | `00f9a54` | **数据发布**：GitHub Release 附件，不表示代码里程碑 |
| Release `dataset-europe-v5` | 附件 `ets2nav-dataset-europe-v5.zip` | 166.4 MB；`gh release list` 标记为 Latest |

**引入两种 tag 的原因**：数据集内容与代码状态是两条独立的演进线——数据集可能在不改动代码的前提下重建（如源数据或编译参数变化），而代码亦可在数据集不变的前提下推进。以单一 tag 体系同时表达二者会导致「tag 指向的数据集与 tag 提交所描述的数据集不一致」的歧义，故数据发布使用独立命名空间 `dataset-*`。

### 发布后复核（本次会话实跑）

下表各项均在**数据集 Release 发布时点**（发布代 `19eceb1`）实跑，是该时点的实际输出而非复述。

| 检查 | 结果 |
|---|---|
| `git status --porcelain -uall` | 空（工作区干净） |
| `git log origin/main..HEAD` | 空（无未推送提交） |
| `git ls-remote origin HEAD refs/heads/main` | 发布时点均为 `19eceb1…`（本地与远程一致） |
| `git ls-remote --tags origin` | 7 个 tag 全部在远程，含 `dataset-europe-v5` |
| Release 附件 | `state=uploaded`，166.4 MB |
| 分发字节一致 | 下载件 SHA-256 `15B03A63…2831BB` == 本地归档（**MATCH**） |
| 解压内容一致 | `routing.graph` SHA-256 `A20CE044…EBA9B8` == `data/europe-v5/routing.graph`（**MATCH**） |

**关于 `main` 的 head 值**：本节刻意不记录具体提交号，因为记录该值的提交本身就会使其失效——此类自指在「文档随代码入库」的工作方式下必然产生。可核对的不变量是：发布代之后的提交**仅**改动 Markdown 文档，以及一处不进入任何门覆盖范围的 `speed-validator` nullable 指令（见下），因此上表结论与 `19eceb1` 这一代码代绑定，不随后续文档提交变化。实际 head 以 `git log -1 --format=%H` 为准；`origin/main` 应与之一致，`git status --porcelain` 应为空。

### 门验证复跑（2026-09-11，确认发布代与验证代一致）

| 门 | 结果 |
|---|---|
| `cargo fmt --check` | PASS（exit 0） |
| `cargo clippy --all-targets` | **0 warnings** |
| `cargo test` | **104 passed / 0 failed**（§9.1 新增 `utc_parts` 3 项 + `SemRecorder` 1 项；§9.2 修正后复跑） |
| `dotnet test map-compiler/MapCompiler.sln` | **80 passed / 0 failed**（8 个项目：HashFs 5 / Sii 22 / Sector 8 / Resource 23 / Definitions 5 / Graph 7 / Prefab 4 / Validation 6） |
| 四套回归套件（修正 §9.2 脚本缺陷后复跑） | `run-p1` / `run-p2` / `run-p3` / `run-p5` **全部 ALL PASS** |

`run-p2` 的逐步输出（§9.2 修复后首次干净通过，可作为「无粘性误报」的对照样本）：

```
[1/7] P1 REGRESSION PASS   [2/7] FMT PASS / CLIPPY PASS / CARGO TEST PASS
[3/7] DATASET SMOKE PASS   [4/7] ROUTE REGRESSION PASS   [5/7] MATCH REPLAY PASS
[6/7] SIGNAL LINK PASS     [7/7] PERF SMOKE PASS
P2 Regression Suite: ALL PASS
```

四套回归套件（`run-p1/p2/p3/p5-tests.bat`）在 `00f9a54` 提交时已实跑 ALL PASS，本次未复跑。依据：本次对产品代码的唯一改动是 `tools/speed-validator/SpeedValidator/Program.cs` 增加一行 `#nullable` 指令（消除 3 处 CS8632 告警，不改变任何运行语义），而 `SpeedValidator` 未包含在 `MapCompiler.sln` 内，亦不被四套件中任何一步调用（已用 `Select-String` 核对 `run-p2/p3-tests.bat` 无 `speed-validator` / `SpeedValidator` 引用）——故该改动不可能影响套件结果。cargo 侧与 dotnet 侧门均已按上表复跑。

### B 侧会话前置条件实测（2026-09-11）

对 runbook §0.1 / §0.3 的前置条件做了实跑核对，避免会话当天才发现工具缺失：

| 检查 | 结果 |
|---|---|
| `nav-core-cli.exe`（Release） | ✅ 存在 |
| `SignalLab.exe` / `SignalLabAnalyze.exe`（Release） | ✅ 存在 |
| `map-inspector.exe`（Release） | ✅ 存在 |
| `SpeedValidator.exe` | ⚠️ **原仅有 Debug 产物**，runbook 使用说明指向 Release 路径——本次补建 Release（`BUILD_EXIT=0`） |
| 插件 DLL（`bin\win_x64\plugins\`） | ✅ `scs-nav-bridge.dll` 139,776 B / `semaphore-bridge.dll` 139,264 B / `ets2la_plugin.dll` 370,176 B |
| **数据集指纹**（runbook §0.3 命令） | ✅ `FINGERPRINT MATCH`（exit 0）——游戏 v1.60.1.7，115 archives / 105 DLC；`MODS total=91 map_altering=1 unprobeable=1 mode=deep`，`MODS-ALTERING promods-eu-map-v281.scs 1051114366 local` |

指纹 MATCH 的含义需精确理解：它说明**自 Europe v5 构建以来，游戏安装与磁盘 mod 集合均未变化**，故数据集与当前游戏地图数据一致。它**不**说明 ProMods 在游戏内未激活——激活集记录在存档内，本项目不解析（§2 残留限制）。这两件事必须分开判断，runbook §0.2 要求的 `game.log.txt` `[mods] Active` 行核对仍然是必要的。

顺带修复：`SpeedValidator` 补建 Release 时暴露 3 处 `warning CS8632`（可空引用类型注解出现在未启用 nullable 的上下文中），以 `#nullable enable annotations` 消除（仅开放注解语法，不启用流分析告警，避免引入新的既有代码告警）。现为 **0 warnings**。该项目不在 `MapCompiler.sln` 内，故不影响既有 dotnet 门口径。

### §9.1 B 侧采集链缺陷（2026-09-11，编制操作流程时发现并修复）

为上一条「B 侧就绪」编制可照抄的操作流程时，对 runbook 中的命令逐条实跑，发现**三个会使 B2 采集产出为零或不可分析的缺陷**。三者均属「文档承诺的行为与实现不符」，且前两项在纸面复核中不会暴露——只有实际执行 `nav-core-cli live` 才会显现。

#### 缺陷 1：`nav-core-cli live` 无法启动（参数守卫误判）

`main` 开头的守卫为 `if args.len() < 3 { usage; exit(2) }`，而 `live` 的 trace 路径是**可选**参数——`nav-core-cli live` 只有 2 个 argv，被误判为「参数不足」直接 `exit(2)` 并打印 `dataset info` 的用法。即 runbook（及 `p2-gameplay-test-checklist`）中给出的这条命令**根本无法运行**：

```
$ nav-core-cli live
用法: nav-core-cli <dataset|info> <dataset-dir>     ← exit 2，未进入遥测
```

**修复**：守卫放宽为 `args.len() < 2`（仅要求存在子命令），并抽出统一的 `usage()` 供前置守卫与未知子命令共用，消除两处文案漂移。其余子命令各自保留 `args.len()` 守卫，参数不足时落到 `_` 分支打印完整用法——行为不变。另加一条：第二参数以 `--` 开头时不当作路径（避免 `live --help` 生成名为 `--help` 的文件）。

#### 缺陷 2：`live` 无参数时不录制任何 trace

`live(trace_path: Option<&str>)` 原实现用 `trace_path.map(...).transpose()`——不传参即 `rec = None`，**一帧都不写**，且不打印任何路径。而文档称「live 模式同时录制 trace，录制路径在启动时打印」（该描述在 `p2-gameplay-test-checklist` 与 runbook 中各出现一次）。

后果的量级：B2 是单趟 20–30 分钟的实机采集，用户按文档执行 `nav-core-cli live`，会话结束时**没有 trace 文件**，T1/T2/T3/T4/T6 全部无法分析——整场会话作废。

**修复**：改为默认必录。省略路径时录到 `%TEMP%\ets2nav-live-<UTC 时间戳>.navtrace`（新增 `default_trace_path()`，含 `utc_parts()` 零依赖 UTC 转换），启动时打印实际路径，并每 60 秒输出 `[rec] 已录制 N 帧 → <路径>` 以便长时驾驶中确认录制在推进。时间戳取到秒，使 T5 的两轮采集产出文件互不覆盖且可区分先后。

关于中断安全性的核实：`TraceRecorder` 写 `File`（`Box<dyn Write>`，无 `BufWriter`），每帧 `write_all` 直达内核，故 `Ctrl+C` 强杀不会丢失已写入的帧——不需要「正常退出」操作。这一性质决定了无需引入信号处理依赖。

#### 缺陷 3：trace 不含信号灯字段，T3 无法离线复核

`TelemetrySnapshot` 的字段为 sequence/running/paused/simulation_time/render_time/game_time/local_scale/rest_stop/position/heading/speed/speed_limit/fuel/job——**没有任何信号灯字段**。而 `next_signal()` 在运行时调 `read_semaphores()` 读取 `Local\ETS2NavSemaphore`；这意味着：

- 实时（`server` + UI）：读到的是**当前**灯态，关联结果正确；
- 离线（`session <trace> <dataset>` 回放）：`read_semaphores()` 读到的是**回放当下**的共享内存（游戏通常已退出 → `None` → 空灯集），**无法重建录制时的信号关联**。

因此 checklist 中「`nav-core-cli session` 输出 upcoming_signal」作为 T3 采集手段是不成立的（`session` 自身确实打印 upcoming_signal，但那是回放环境的值，不是采集时的值）。

**修复**：`live` 增加信号灯**旁路记录**——每 20 Hz 采样 `read_semaphores()`，写入与 trace 同名的 `<trace>.sem.csv`（长表：`wall_ms,slot,id,kind,state,time_remaining,x,y,z,qx,qy,qz,qw`，每次采样每灯一行）。设计取舍：

| 决策 | 理由 |
|---|---|
| 旁路文件而非扩展 trace 格式 | 扩 `TelemetrySnapshot` 会牵动 dataset/replay/match/session 全链路兼容性；T3 只需旁路证据 |
| 仅在确实读到共享内存后才创建文件 | 无桥接器/无信号灯时不产出空文件，避免把「未采到」误读成「采到了但无灯」 |
| 20 Hz 采样 | 灯态与倒计时以秒为尺度变化，20 Hz 远高于必要精度 |
| 写 `File` 而非 `BufWriter` | 与 trace 同策略：`Ctrl+C` 不丢数据 |
| 长表而非宽表 | 灯数随路口变化（槽位上限 64），长表可直接按 `id`/时间窗切片 |

#### 验证

| 项 | 结果 |
|---|---|
| 参数路径实跑（6 项） | `live` 无参 → 启动+录制+打印路径；`live <路径>` → 录到指定文件；无参整体 → 用法+exit 2；`replay` 参数不足 → 用法+exit 2；未知子命令 → 用法+exit 2；`live --help` → 走默认路径且**未**生成名为 `--help` 的文件 |
| **信号通路端到端**（合成共享内存） | 用 .NET `MemoryMappedFile` 构造 `Local\ETS2NavSemaphore`（header 16B + 2 个 48B 灯槽，写入 id=42/kind=1/state=2/time=12.250/pos=(1.5,2.5,3.5)/quat y=w=sin45°，以及 id=7/kind=0/state=0/time=3.500/pos=(-4,0,8)），在**无游戏、无遥测**条件下运行 `live`：文件被创建、路径被打印、7 次采样 × 2 灯 = 14 数据行，逐字段与写入值一致（`12.250`、`1.500`、`0.70711` 等） |
| 阴性对照 | 无桥接器环境下运行 `live` → trace 文件创建（0 字节，无遥测帧），`*.sem.csv` **数量为 0**（符合「未采到不产空文件」的设计） |
| `utc_parts` 单测 | 4 组基准值交叉核对（`0`→1970-01-01、`1767225600`→2026-01-01、`1789058002`→2026-09-10T16:33:22Z、闰日 `1709251199`→2024-02-29T23:59:59Z、年末 `1735689599`→2024-12-31T23:59:59Z） |
| `SemRecorder` 单测 | 表头与列数（13）、`slot` 序号、`id`、`time_remaining`/坐标/四元数小数位、派生路径 `x.navtrace` → `x.sem.csv` |

其中端到端一项的意义在于：它不依赖游戏，直接验证「共享内存存在 → 文件产出 → 内容正确」这条完整通路，因此把 T3 的可分析性从「依赖实机才能确认」变为「已在离线环境证明」。样本量说明：该次 4 秒内仅 7 次采样，原因是无遥测时 `Disconnected` 分支有 500 ms 退避、循环随之降频；实际游戏中遥测正常（~60 Hz 轮询），20 Hz 采样成立。

**一处设计修正（实跑中发现）**：信号采样最初写在 `Fresh(snap)` 分支内，意味着遥测一旦进入 `Stale`/`Disconnected`，信号记录也随之停止。但遥测桥与信号桥是两个独立插件，信号证据不应依赖遥测状态——已将该采样移至 `match` 之外，使其独立于遥测状态。上述端到端测试正是在**完全无遥测**的条件下通过，可直接证明该解耦生效。

> 记一次自查失误：`utc_parts` 首版单测我把 `16:33:22` 的秒位误写为 `2`，测试失败后先怀疑算法。手工验算（`rem=59602 → 16h33m22s`）确认算法正确、期望值笔误，修正后通过。若当时直接改算法去迁就错误期望，会引入真实缺陷。

### §9.2 回归套件「粘性 FAIL」缺陷（2026-09-11，由一次误判排查暴露）

#### 现象

修完上节三项缺陷后重跑四套件，`run-p2-tests.bat` 报出 6 项失败：

```
=== [2/7] cargo fmt/clippy/test ===
FMT PASS
CLIPPY FAIL
CARGO TEST PASS
=== [3/7] dataset v2 smoke ===
DATASET SMOKE FAIL
=== [4/7] route regression ===
ROUTE REGRESSION FAIL
...（[5/7]~[7/7] 同为 FAIL）
```

表面看是「clippy 一挂、全链崩塌」。第一反应是**并发干扰**——我当时正在同一 `nav-core` 目录跑 `cargo build --release`，与套件的 cargo 操作争 `target` 锁。

#### 实际是两个独立问题叠加

**其一，`CLIPPY FAIL` 是真的，且是我的代码引起。** 排除并发后单独复跑，clippy 报：

```
error: approximate value of `f{32, 64}::consts::FRAC_1_SQRT_2` found
error: could not compile `nav-core-cli` (bin "nav-core-cli" test) due to 2 previous errors
```

根因是新写的 `SemRecorder` 单测里用了字面量 `0.7071`（作为 90° 偏航四元数的 y/w 分量），触发 `clippy::approx_constant`。**该 lint 正是为这类「近似常数」而存在**，属于应当修正的写法。改法不是压制告警，而是直接用 `std::f32::consts::FRAC_1_SQRT_2`——它同时更准确地表达了「sin45°」这一语义。

顺带暴露我流程上的疏漏：新增测试代码后我按 `cargo test` 验证通过即继续，**未重跑 clippy**（`--all-targets` 才覆盖 test target）。`cargo test` 通过不代表 clippy 通过。

**其二，`[3/7]`~`[7/7]` 的 FAIL 全是误报，属脚本缺陷。** 三个套件（`run-p2`/`run-p3`/`run-p5`）的每步结论写成：

```bat
if errorlevel 1 set FAIL=1
if %FAIL%==1 (echo DATASET SMOKE FAIL) else (echo DATASET SMOKE PASS)
```

判断条件是**累积标志** `FAIL` 而非该步自身的 `errorlevel`——第一步失败后，`FAIL` 永久为 1，后续每步都打印 FAIL，**无论其命令是否真的成功**。于是单个 clippy 错误被放大成「六项失败」，把真实故障面掩盖成一片。

这与此前修过的两处套件可复现性缺陷（`run-p2` 对 `%TEMP%\real.navtrace` 的隐式依赖、`run-p1` 依赖未纳入解决方案的 Debug 二进制）同属**验证完整性缺陷**：套件本身给出误导性结论，而非产品缺陷。

#### 修复

三套件统一改为每步独立判定，`FAIL` 仅用于最终退出码：

```bat
if errorlevel 1 (echo DATASET SMOKE FAIL & set FAIL=1) else (echo DATASET SMOKE PASS)
```

`run-p1-tests.bat` 原本即为该写法（0 处粘性判断），未改动。另修 `run-p5` 的 `[3/4]`：原逻辑在基线存在时也打印 `BASELINE GEN PASS`（同样基于累积标志），现改为基线存在时输出 `BASELINE EXISTS`、缺失时才生成并报告——使「跳过」与「生成成功」可区分。

> **对历史记录的影响**：`p5-fix-closeout-check-2026-08.md` §三记录的 `[3/4] BASELINE GEN PASS` 是该次实跑（基线已删除后重生成）的真实输出，属历史证据，不作改动；但此后基线存在时该行输出为 `BASELINE EXISTS`，与此前不同，属预期变化。

#### 方法论教训

两个问题叠加时，「第一个失败 + 后续全挂」的形态很容易被归因为单一原因（此处是并发）。若当时接受「并发干扰」这一解释并直接重跑，`approx_constant` 会被下一次套件运行重新捕获（因为代码未改），但**若并发恰好不再发生、且 clippy 因增量缓存未重跑，则可能长期潜伏**。区分二者的关键动作是：在排除干扰的条件下单独复跑失败项，并读取其**原始输出**（而非套件的汇总标签）。

#### 修复后复跑

| 套件 | 结果 | 逐步输出 |
|---|---|---|
| `run-p1-tests.bat` | **ALL PASS** | 6 步全 PASS（unit 80 / map-inspector build / Berlin gate 95.4% / Germany gate 96.4% / determinism / Rust reader / Europe scale `failed_prefabs: 0`） |
| `run-p2-tests.bat` | **ALL PASS** | 7 步全 PASS（P1 链 + FMT/CLIPPY/CARGO TEST + DATASET SMOKE + ROUTE REGRESSION + MATCH REPLAY + SIGNAL LINK + PERF SMOKE） |
| `run-p3-tests.bat` | **ALL PASS** | 4 步全 PASS（P2 链 + FMT/CLIPPY/CARGO TEST + SPEED LOOKAHEAD + CAMERA VERDICT） |
| `run-p5-tests.bat` | **ALL PASS** | 4 步全 PASS（CARGO TEST / ROUTER TEST / FMT / CLIPPY / BUILD / `BASELINE EXISTS` / OD REGRESS / OD CHECK） |

对照 §9.2 开头的失败输出可见：修复前 6 项 FAIL，修复后 7 项 PASS——其中 6 项从未真正失败过。

### §9.3 GitHub 同步（2026-09-11 第二轮）

§9.1/§9.2 的修复与文档改动经 `bdbd7ea` 推送 `main`。同步后复核：

| 检查 | 结果 |
|---|---|
| `git push origin main` | `8ea6fc4..bdbd7ea` |
| `git rev-parse HEAD` == `origin/main` | 一致 |
| `git status --porcelain -uall` | 空 |
| `git log origin/main..HEAD` | 空（无未推送提交） |
| 远程 tag | 7 个（`v0.1.0-p0` ~ `v0.6.0-p4p5p6` + `dataset-europe-v5`），与本地一致 |
| Release `dataset-europe-v5` | `state=uploaded`，`draft=False`，标记 Latest（数据集内容不受本轮代码改动影响，无需重发） |

**数据集是否需重发——判定与依据**：本轮改动全部位于 `nav-core/tools/nav-core-cli`（CLI 工具）与批处理脚本，**未触及 map-compiler 的任何代码路径**，故数据集内容不变。已用 `--check-fingerprint` 佐证（§9.1 实测 `FINGERPRINT MATCH`），且 Release 附件的两个 SHA-256（归档 `15B03A63…`、`routing.graph` `A20CE044…`）仍与本地一致。因此不重发 Release，仅推送代码与文档。

### 本轮文档同步清单（2026-09-11，累计两轮）

第一轮（§9，提交 `c5fe3d7` ~ `8ea6fc4`）：

| 文件 | 变更 |
|---|---|
| `PLAN.md` | §1 最后更新与摘要（数据集分发、B 侧就绪核对、发布后复核）；§5 tag 表新增 `dataset-europe-v5` 并说明两种 tag 命名空间的分工 |
| `PLAN-P3plus.md` | §5 末尾新增「B 侧就绪条件」表（手册 / 插件 / 工具 / 数据集 / 阻塞风险） |
| `README.md` | 当前阶段补同步日期与存量门复核结果；新增「验证与复现入口」节（四套件 + cargo/dotnet 命令 + 数据集获取途径）；目录结构更新（补 `nav-core` / `desktop` / `data` 与测试计数 65 → 80） |
| `p5-fix-closeout-check-2026-08.md` | §5 追加后续推进注（提交链延伸至 `19eceb1`、新增数据发布 tag） |
| 本文件 | §9 新建（GitHub 同步记录） |

第二轮（§9.1 ~ §9.3，提交 `bdbd7ea`）：

| 文件 | 变更 |
|---|---|
| `nav-core/tools/nav-core-cli/src/main.rs` | 采集链缺陷 1/2/3 修复（参数守卫、默认录制、信号旁路记录）+ 4 项新增单测 |
| `run-p2-tests.bat` / `run-p3-tests.bat` / `run-p5-tests.bat` | 逐步判定与累积 `FAIL` 解耦（§9.2）；`run-p5` `[3/4]` 区分「基线存在」与「生成成功」 |
| `b-session-runbook-2026-08.md` | §一 补首行判据与 trace 路径说明；§二 补两终端分工、T3 停车等待、T5 两轮 DLL 装卸、交付项；§四 补各 T 项输入；§五 新增测量条件 6/7；新增 §六 命令速查 |
| `p2-gameplay-test-checklist-2026-08.md` | 修正 `session` 语义（离线回放而非实时快照）与 T3 采集手段；补 live 默认录制路径 |
| `PLAN.md` | §1 最后更新与摘要补采集链缺陷；测试计数 100 → 104 |
| `PLAN-P3plus.md` | §5「B 侧就绪条件」表新增「采集链缺陷」行；补命令速查与 `speed-validator` Release 说明 |
| `p5-closeout-2026-08.md` | §5 第 5 项补记同类第三项验证完整性缺陷（粘性 FAIL） |
| `README.md` | 「验证与复现入口」补 `ETS2_INSTALL` 前置（`run-p1` 不自设该变量）；结果记录更新为四套件已复跑 ALL PASS + cargo 104 测试 |
| 本文件 | §9.1（B 侧采集链缺陷）、§9.2（套件粘性 FAIL）、§9.3（本轮 GitHub 同步） |

### 对既有文档的连带修正

`p2-gameplay-test-checklist-2026-08.md` 与 `b-session-runbook-2026-08.md` 中失实的描述已同步：前者修正 `session` 的语义（离线回放而非实时快照）与 T3 采集手段，后者补齐两个终端的分工、T5 两轮的 DLL 装卸步骤、`*.sem.csv` 交付项与命令速查节。

**未改动项与其理由**：`PLAN.md` §4.2「验证」段的「clippy 0 / 97 测试」与本文 §5 的「cargo test 100 passed」均为**各自时点的历史实跑记录**（分别为 P5 修复时与 2026-08-12 加固时），不是当前值，故不追改——文档记录当时证据，当前值由 §9 各表与本清单承担。



---

## 附录 A —— 2026-09-12 追补（P4R Batch 4）

**本节为事后追补，不修改上文任何当时的结论与数值。**

上文 §9.2 记录了「逐步判定与累积 FAIL 解耦」这一修复，并据此在 §5 的四套件表中声明各步骤判定系基于真实退出码。按 P4R Batch 4 的重新审计，该声明对其中若干步骤并不成立：

1. **管道吞掉退出码。** `run-p2/p3-tests.bat` 的 `cargo clippy --all-targets 2>&1 | findstr …`、`cargo test 2>&1 | findstr /C:"FAILED"`，以及 `nav-core-cli regression|match|signal|bench … | findstr …`，其 `ERRORLEVEL` 取的是 `findstr` 的退出码，被测进程退出码被丢弃。于是「CARGO TEST PASS」在 `cargo test` 因**编译错误**失败（输出为 `error[E…]`、不含 `FAILED`）时同样成立——编译不过被报告为测试通过。
2. **反向判定。** `run-p5-tests.bat` 的 `cargo test -p od-corpus 2>&1 | findstr /C:"FAILED" /C:"error[E"` 之后接 `if errorlevel 1 (echo … PASS)`：`findstr` 退出码 1（未找到）被当作成功条件；`cargo` 无法启动时输出为空，同样判 PASS。
3. **恒真 marker。** `nav-core-cli match` 的统计行与 `signal` 的空结果分支都含有被 `findstr` 搜索的关键词，文本判定无法区分成功与空结果。
4. **基准自证。** `run-p5-tests.bat` 在基准缺失时先生成、再与刚生成的文件比较，结构上不可能失败；因此该文件的 ALL PASS 记录不构成对 OD 一致性的验证。
5. **隐式输入与陈旧产物。** `run-p2/p3` 复用 `%TEMP%\real.navtrace`（跨轮污染）；`run-p1` 执行 `tools/dataset-reader-smoke/target/release/*.exe` 却从未构建它；`run-p2/p3/p5` 在 trace 已存在时不重建 `nav-core-cli`。
6. **开发者绝对路径。** `run-p2/p3/p5` 以 `set DATASET=E:\Projects\Pi\ETS2Nav\data\europe-v5` 覆盖用户环境变量；产品代码中另有六处本机绝对路径缺省值，测试代码中另有三处。

上述各项均已在 P4R Batch 4 中修复并重新实测（四条入口 exit 0）。**上文四套件 ALL PASS 的记录本身未被推翻**——它记录的是当时那些命令确实执行完毕；被修正的是「该记录证明了什么」这一推论强度。当前判定契约、配置 contract、clean-clone 复现与 E2E-09 根因见 `docs/validation/p4r-batch4-2026-09.md`。