# ETS2Nav — Euro Truck Simulator 2 外部智能导航系统

独立于游戏原生 Route Advisor 的外部智能导航系统：自行解析 ETS2 地图资源建立导航数据库，提供独立路径规划、实时地图匹配、转向导航、红绿灯倒计时（±1 s 目标）、限速与测速提示、POI 搜索、中文语音导航，PC 核心 + PC/移动端双前端。

- **需求与技术基线**：[Euro Truck Simulator 2 外部智能导航系统-v0.2.md](./Euro%20Truck%20Simulator%202%20外部智能导航系统-v0.2.md)
- **可行性评估**：[ETS2 外部智能导航系统可行性评估.md](./ETS2%20外部智能导航系统可行性评估.md)
- **执行计划与进度**：[PLAN.md](./PLAN.md)
- **当前阶段**：**P0~P6 A 侧全部关门（2026-08-12，tag `v0.6.0-p4p5p6` @ `7900bbc`；2026-09-11 完成 GitHub 与文档同步复核 + B 侧采集链缺陷修复）**——P3 关门 tag v0.4.0-p3；A2~A5（P4 UI / P5 OD corpus / P6 性能评估 / B7 工具）tag v0.5.0；此后完成 **P5 图缺陷根因排查与修复**（UK 孤立与 ferry 悬空为真实编译器缺陷——主分量 75.78%→79.35%、transit 端点孤立 258→0、od-check uturns 1→0）、**离线加固四项**（UI 链断言不稳定、§9 mod 指纹漏报、§57 规范违背、UI 事件分发）与 **B 侧采集链三项缺陷修复**（`live` 无法启动 / 不录制 trace / trace 无信号灯数据），run-p1/p2/p3/p5 四套件 ALL PASS。存量门复核：cargo 104 测试 + dotnet 80 测试（8 项目）全绿。**唯一剩余工作为 B 侧实机测试**（runbook 已就绪）。
- **关门报告**：P4 / P5 / P6 见 [docs/validation/](./docs/validation/)（`p4-closeout` / `p5-closeout` / `p6-closeout-2026-08.md`）；P5 修复根因与验证见 `p5-graph-defects-2026-08.md` / `p5-graph-defects-verification-2026-08.md`；离线加固见 `p4-p6-hardening-2026-08.md`
- **B 侧会话操作手册**：[b-session-runbook-2026-08.md](./docs/validation/b-session-runbook-2026-08.md)（含 mod 激活集核对）
- **数据集**：[Release `dataset-europe-v5`](https://github.com/HaowenCang/ETS2Nav/releases/tag/dataset-europe-v5)（166 MB 归档）——**无需拥有游戏**即可运行导航核心与 UI 联调。本地重建（需游戏，约 4 分钟）见 `docs/validation/p5-closeout-2026-08.md` §6。注意数据集受 `.gitignore` 约束不入库。

## 架构概览（v0.2 §3）

```
ETS2 ──SCS Telemetry SDK──▶ Telemetry Bridge (C++ DLL) ──Shared Memory──▶ Navigation Core (Rust)
                                                                              │ HTTP/WebSocket
                                                                              ▼
                                                                  Windows UI / Mobile / Browser

ETS2 Files ──▶ Map Compiler (C#/TS，离线低频) ──▶ map.db / routing.graph / junction.graph / search.db / map.pmtiles
```

## 开发状态

见 [PLAN.md §3 任务状态表](./PLAN.md)。

## 验证与复现入口

前置：数据集不入库，须先构建 `data/europe-v5`（约 4 分钟，需已安装 ETS2），或从 [Release `dataset-europe-v5`](https://github.com/HaowenCang/ETS2Nav/releases/tag/dataset-europe-v5) 取得后解压至 `data/europe-v5`。构建命令见 `docs/validation/p5-closeout-2026-08.md` §6。

判定逻辑集中在一处：`scripts/regression.ps1`。四个 `run-p*-tests.bat` 是它的薄包装（只做编码前置检查、转发参数、原样传播退出码）；P2 链内含 P1，P3 链内含 P2。

**云端门与本地完整门是两件事。** P1/P2/P3 需要正版 ETS2 游戏资源，而游戏资源不得下载、不得以伪造的 `ETS2_INSTALL` 顶替、也不得以空 fixture 代替，因此它们只属于本地与发布的完整门。GitHub Actions 执行的是可诚实复现的那一部分：

| 门 | 需要游戏资源 | 需要数据集 | 云端 |
| --- | --- | --- | --- |
| `-Suite Portable` | 否 | 否 | 是 |
| `-Suite CLI` | 否 | 是 | 是 |
| `-Suite P5` | 否 | 是 | 是 |
| `-Suite P1` / `P2` / `P3` | 是 | P2/P3 是 | 否 |
| `-Suite SelfTest` | 否 | 否 | 是 |

`Portable` 覆盖 cargo fmt/clippy/test、map-compiler 的 portable dotnet 测试（显式排除 `GameAssetsRequired` 分类并断言执行数）、前端 clean build 两轮哈希比对与 `build-manifest.json` 校验、Desktop 编译门（必须在 `dist/` 生成之后）以及 harness 自检。CI 定义见 `.github/workflows/ci.yml`。

四个 workflow job 的显示名即建议用作 `main` 保护检查的名称：`Source Gates`、`Dataset Gates`、`Web E2E`、`Security Portable`。名称里不含矩阵或版本，可长期稳定引用。`Security Portable` 这个后缀是有意的：该 job 真实门控的是**协议集成**（`test:protocol`）与**浏览器回环安全**（`test:browser-loopback`，BLS-01..09，真实 Chromium + 真实攻击页面），两者都不是完整 S1–S11 局域网矩阵。S9（不受允许来源在连接层被拒绝）需要一个非 RFC1918 的真实对端地址，hosted runner 不具备，脚本因此报 `NOT VERIFIED` 并以非零退出；该矩阵在 job 内仍原样运行（`continue-on-error`，不删断言、不改写结论），完整形态属本地/发布门：本机实测 `FAIL=0 NOT_VERIFIED=0 SKIP=0`。因此 `Security Portable` 通过只声称 portable protocol + browser-loopback，不代表完整局域网矩阵。

```bat
:: 游戏安装目录必须显式提供：回归入口不猜测本机位置
set ETS2_INSTALL=<ETS2 游戏根目录>
:: 以下可选，缺省为 repo 内相对路径
set ETS2NAV_EXTRACTED=<scs_extractor 解包根，含 base_map\ 与 def\>   :: 缺省 <repo>\vendor\extracted
set ETS2NAV_DATASET=<数据集目录>                                    :: 缺省 <repo>\data\europe-v5
set ETS2NAV_OD_BASELINE=<OD 基准文件>                               :: 缺省 <repo>\od-baseline-europe-v5.txt

run-p1-tests.bat        :: P1：unit / map-inspector 构建 / Berlin+Germany gate / determinism / Rust reader / Europe scale
run-p2-tests.bat        :: P2：链内 P1 + cargo 门 + dataset smoke + route regression + match replay + signal link + perf smoke
run-p3-tests.bat        :: P3：链内 P1 → P2 → P3（speed lookahead + camera verdict）
run-p5-tests.bat        :: P5：OD 回归 + OD 检查（基准缺失即 PRECONDITION FAIL，绝不自动生成）

:: 等价的规范入口（与 run-p*.bat 同一实现）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\regression.ps1 -Suite All
:: 云端可执行门（不需要游戏资源）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\regression.ps1 -Suite Portable
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\regression.ps1 -Suite P5,CLI -Dataset <数据集目录>
:: 基准更新是独立的维护动作（不是测试）；覆盖已有基准需额外 -Force
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\regression.ps1 -Suite P5 -UpdateBaseline -Force
:: harness 假绿自检（H1–H6）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\regression.ps1 -Suite SelfTest

:: 退出码：0=PASS 1=TEST FAILURE 3=PRECONDITION FAILURE（外部输入缺失）4=HARNESS FAILURE
:: 外部输入可改用参数传入：-Ets2Install / -Ets2Extracted / -Dataset / -OdBaseline
:: CI 永不使用 -UpdateBaseline；基准不一致一律 FAIL 交人审查。

cd nav-core && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
:: portable 子集（排除需要游戏资源的 GameAssetsRequired 分类）：
dotnet test map-compiler/MapCompiler.sln --filter "Category!=GameAssetsRequired"
:: 完整门（需 ETS2_INSTALL + ETS2NAV_EXTRACTED，缺失即 FAIL 而不是静默通过）：
dotnet test map-compiler/MapCompiler.sln
```

工具链契约固定在仓库内，不依赖"runner 今天预装了什么"：`rust-toolchain.toml`（Rust 1.96.0 + rustfmt/clippy）、`global.json`（.NET SDK 9.0.x）、`.nvmrc`（Node 24.13.0）。行尾契约见 `.gitattributes`：文本一律 LF 入库，`.bat`/`.cmd` 检出为 CRLF（cmd.exe 对 LF-only 批处理的解析不可靠）。


### Web UI（正式前端）

`tools/ets2nav-web/` 的静态产物不入库，须由锁定版本的依赖构建；clean clone 同样按以下步骤恢复，无需人工复制任何文件：

```bat
cd tools\ets2nav-web
npm ci                 :: 依 package-lock.json 恢复确定版本依赖（maplibre-gl / pmtiles / qrcode / @playwright/test）
npm run build          :: 生成 dist\（vendor 库 + 页面 + build-manifest.json）
```

产物为 `tools/ets2nav-web/dist/`，即 nav-server 的默认 web root（可用 `--web=` 覆盖）与 Desktop 的 `frontendDist`。
`map.pmtiles` 与 `fonts/` 属运行期可选资源：缺失时前端进入无底图模式（或跳过 city 文字层），并在 console 输出 INFO/WARN，不静默失败。
依赖版本、来源与产物 SHA-256 记录于 `dist/build-manifest.json`。

### 测试入口（P4R Batch 2 起）

两层测试职责分离，二者不可互相替代：

```bat
cd tools\ets2nav-web
npm run test:protocol  :: nav-server 协议集成测试（HTTP 路由 / WS 帧 / 快照通道 / 事件类型白名单）
npm run test:protocol -- --selftest   :: 仅跑 remaining 推进判定的确定性回归用例（不需要服务器）
npx playwright install chromium        :: 首次：下载本仓库锁定版本对应的 Chromium
npm run test:e2e       :: Browser E2E：真实 Chromium 加载 index.html + app.js + MapLibre
                       :: + pmtiles protocol + QRCode + HTTP + WS，断言 DOM / source-layer / console
```

`verify-server-protocol.py` **不是** browser E2E——它以原始 socket 驱动 HTTP 与 WebSocket，
不启动浏览器、不执行 app.js、不触碰 DOM；其定位是协议集成测试（历史上曾被称作
「UI 链验证」，该表述已更正）。真实浏览器断言一律由 Playwright 套件承担。
E2E 使用隔离环境（临时 web root + 由正式 `PmtilesWriter` 现场生成的合法 PMTiles + 动态端口
+ 确定性合成 trace），不读取源码目录中的运行期 `map.pmtiles`。

结果记录（2026-09-11，**验证时点** `bdbd7ea`；该 hash 说明验证在哪个代码代上执行，非当前 head）：四套件全部 ALL PASS（`BAT_EXIT=0` 复核）——`run-p1` 6 步、`run-p2` 7 步、`run-p3` 4 步、`run-p5` 4 步逐步 PASS；cargo fmt PASS / clippy **0 告警** / **104 测试通过**；dotnet **80 测试通过**（8 项目）。逐项实跑输出、GitHub 发布复核与本次修复的三项采集链缺陷见 `docs/validation/p4-p6-hardening-2026-08.md` §9/§9.1/§9.2。

## 目录结构（P0 精简版，完整版见 v0.2 §72）

```
ets2-nav/
├─ telemetry-plugin/    # C++ DLL：SDK 1.14 + 共享内存（scs-nav-bridge / semaphore-bridge）
├─ map-compiler/        # 地图解析与编译（C#，独立实现）→ routing/junction graph、map.db、search.db、pmtiles
│  └─ tests/            # 8 个测试项目，80 测试
├─ nav-core/            # Rust 导航核心：crates/（dataset/graph/spatial/matcher/router/telemetry）+ tools/（nav-core-cli、od-corpus）
├─ tools/               # 工具：map-inspector / graph-debugger / telemetry-dump / signal-lab / speed-validator / ets2nav-web …
├─ desktop/             # Tauri 2 桌面端
├─ docs/                # 格式笔记（format-notes）、决策记录（decisions）、验证报告（validation）
├─ data/europe-v5/      # 规范数据集（不入库；重建或从 Release 获取）
└─ PLAN.md              # 执行计划与进度
```

## 许可证

**GPL-3.0**（2026-08-09 决定：项目改为 GPL 开源，以允许复用 GPL 生态实现——TruckLib/ETS2LA/TruckSim Maps）。

历史：v0.1 阶段曾定为 MIT；2026-08-09 修订为 GPL-3.0。
