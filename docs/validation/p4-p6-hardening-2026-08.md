# P4/P6 离线加固验证记录（2026-08-12）

**范围**：审计阶段登记为「不强制修复」的遗留项中可离线处理者——UI 链验证不稳定、§9 mods 指纹漏报、§57 自动缩放/自动恢复、UI 事件类型分发；外加数据集外部分发与 B 侧会话准备。
**性质**：前四项中**三项经复核为真实缺陷（含两项规范违背）**，非风格问题；详见各节「性质」标注。

---

## §1 verify-ui-chain.py「remaining 递减」断言不稳定

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

扩展 `verify-ui-chain.py`：事件类型白名单断言（只允许 `vehicle`/`map_state`）+
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
| verify-ui-chain.py | 8/8 PASS（扩展后 4 项新增断言全过） |

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
| `cargo test` | **100 passed / 0 failed**（9 个 test target） |
| `dotnet test map-compiler/MapCompiler.sln` | **80 passed / 0 failed**（8 个项目：HashFs 5 / Sii 22 / Sector 8 / Resource 23 / Definitions 5 / Graph 7 / Prefab 4 / Validation 6） |

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

### 本轮文档同步清单（2026-09-11）

| 文件 | 变更 |
|---|---|
| `PLAN.md` | §1 最后更新与摘要（数据集分发、B 侧就绪核对、发布后复核）；§5 tag 表新增 `dataset-europe-v5` 并说明两种 tag 命名空间的分工 |
| `PLAN-P3plus.md` | §5 末尾新增「B 侧就绪条件」表（手册 / 插件 / 工具 / 数据集 / 阻塞风险） |
| `README.md` | 当前阶段补同步日期与存量门复核结果；新增「验证与复现入口」节（四套件 + cargo/dotnet 命令 + 数据集获取途径）；目录结构更新（补 `nav-core` / `desktop` / `data` 与测试计数 65 → 80） |
| `p5-fix-closeout-check-2026-08.md` | §5 追加后续推进注（提交链延伸至 `19eceb1`、新增数据发布 tag） |
| 本文件 | §9 新建（GitHub 同步记录）；§8 关联文档清单同步 |

