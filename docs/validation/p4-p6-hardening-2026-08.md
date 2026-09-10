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
