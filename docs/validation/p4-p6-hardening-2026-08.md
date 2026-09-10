# P4/P6 离线加固验证记录（2026-08-12）

**范围**：审计阶段登记为「不强制修复」的遗留项中，四项可离线处理者——UI 链验证不稳定、§9 mods 指纹漏报、§57 自动缩放/自动恢复、UI 事件类型分发。
**性质**：其中**三项经复核为真实缺陷（含两项规范违背）**，非风格问题；详见各节「性质」标注。

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
| `run-p1/p2/p3-tests.bat` | ALL PASS（见 §6 实跑记录） |
| `run-p5-tests.bat` | ALL PASS |
| verify-ui-chain.py | 8/8 PASS（扩展后 4 项新增断言全过） |

## §6 关联文档更新

- `p4-ui-2026-08.md` §六：三项 MINOR 状态更新（不稳定断言 / §57 / 事件分发）
- `p4-closeout-2026-08.md` §5：已知限制第 5~8 项销账
- `p6-performance-eval-2026-08.md` §2.3/§五：§9 覆盖 4/9 → 6/9（mods 集合与 mod 顺序由 mod 指纹覆盖；内容哈希可选）
- `p6-closeout-2026-08.md` §5：已知限制第 1 项更新
