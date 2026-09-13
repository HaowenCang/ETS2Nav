# ETS2Nav — Euro Truck Simulator 2 外部智能导航系统

独立于游戏原生 Route Advisor 的外部智能导航系统：自行解析 ETS2 地图资源建立导航数据库，提供独立路径规划、实时地图匹配、转向导航、红绿灯倒计时（±1 s 目标）、限速与测速提示、POI 搜索、中文语音导航，PC 核心 + PC/移动端双前端。

- **需求与技术基线**：[Euro Truck Simulator 2 外部智能导航系统-v0.2.md](./Euro%20Truck%20Simulator%202%20外部智能导航系统-v0.2.md)
- **可行性评估**：[ETS2 外部智能导航系统可行性评估.md](./ETS2%20外部智能导航系统可行性评估.md)
- **执行计划与进度**：[PLAN.md](./PLAN.md)
- **当前阶段**：**A 侧核心关门（2026-08-12，tag `v0.6.0-p4p5p6` @ `7900bbc`），P4R 批次进行中**。状态分三层，各自只声称已验证的部分：
  - **核心与 CI 加固**：P0~P6 A 侧全部关门；P4R Batch 2–5.5 完成远端确定性、安全硬门恢复与四 job required CI（`main` 已启用分支保护）。Batch 6A（2026-09-13）关闭 MapLibre critical advisory、建立 nav-server 数据源 worker 故障遏制、把 Desktop 变为自带 sidecar 的桌面产品边界、并解决 telemetry plugin 的 SDK provenance 与可复现构建。逐轮证据见 `docs/validation/p4r-batch*.md`。
  - **发布工程**：**未完成**。正式 GitHub Release 与版本 tag 尚未创建；发布包（bundle）已可组装与校验，但正式欧洲底图 `map.pmtiles` 与字形 `fonts/` 尚未随包分发（两者缺失时前端降级：无底图 / 跳过 city 文字层），因此当前不得声称"完整离线导航 UI"。SCS telemetry SDK 不随仓库分发，构建插件 DLL 前须先执行 `scripts/prepare-scs-sdk.ps1`（外部前置）。
  - **实机验证**：**未开始**。B1–B6 实机驾驶验证尚未执行（runbook 已就绪），因此所有导航结论仍限于合成轨迹与数据集内回归。
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
| `-Suite FaultContainment` | 否 | 是 | 是 |
| `-Suite Desktop` | 否 | 是 | 是 |
| `-Suite JobObject` | 否 | 是 | 是 |
| `-Suite P5` | 否 | 是 | 是 |
| `-Suite P1` / `P2` / `P3` | 是 | P2/P3 是 | 否 |
| `-Suite SelfTest` | 否 | 否 | 是 |

`-Suite Release`（Batch 6B 引入）同样不需要游戏资源、需要数据集，但**不进 required CI**：它把 373 MB 数据集完整打包并解压两轮做可复现性对照，成本远高于其边际检出能力，而数据集本身已由 `Dataset Gates` 下载并校验。发布证据链由它在本机产出并写进当轮报告。

`Portable` 覆盖 cargo fmt/clippy/test、map-compiler 的 portable dotnet 测试（显式排除 `GameAssetsRequired` 分类并断言执行数）、前端 clean build 两轮哈希比对与 `build-manifest.json` 校验、**MapLibre 版本门**（离线判定已安装版本 ≥ 安全下限 `6.4.1` 且与 lockfile、精确钉版一致，advisory 下限写成脚本内常量，不随 advisory 数据库变化）、**telemetry plugin 产物身份检查**（PE x64 DLL、导出齐备、镜像内无开发机绝对路径）、**SCS SDK provenance 契约核对**（离线：URL/文件名/SHA-256/必需头/许可，不联网）、Desktop 编译门以及 harness 自检。

`FaultContainment` 主动注入数据源 worker 故障（panic 与意外返回两种），验证进程不以「listener 仍接受连接、状态永久冻结」的 zombie 形态存活：退出码 70、stderr 带致命类别且不含令牌、已连接的 WS 客户端被显式断开、降级窗口内一律 503。注入钩子只在 debug 构建存在；release 二进制不含任何注入环境变量字面量，由 `CLI` 套件以字节扫描断言。

`Desktop` 从 clean build 出发组装发布 bundle（release 的桌面壳 + sidecar + 前端产物 + 数据集），独立复核每个身份后，用 **bundle 内**的 Desktop 真实跑一遍生命周期：身份校验、运行时端口协商、可服务、退出回收、无孤儿（含被强杀的情形，由作业对象保证）。窗口形态（真实 WebView 渲染 UI 并连上 WS）需要在交互式桌面上运行 `--windowed`，不在云端步骤内。

`JobObject` 判定**作业对象不可用时的 fail closed**：`create` 或 `assign` 失败即启动失败、sidecar 被立即回收、退出码 24（`kind=sidecar-job-required`）、且不建窗口。判定对象是 `--release --features fault-inject` 的构建——注入代码默认不编译进发布产物，而「release 在作业对象缺失时会怎样」这条判据若只在 debug 上验证，就只是一条只在测试里存在的旁路。发布产物本身不含注入面，由 `assemble-bundle.ps1` 在组装时按字节扫描断言。开发者可用 `--allow-no-job-object` 显式降级，此时必须留下 WARN 且状态可观测。

`Portable` 另含**发布隐私扫描器自检**：用一个临时树同时放正样本（每种泄漏模式各一）与负样本（`http://`、`https://`、文档中的回环默认端口都不得被误判），逐条核对分类结果。扫描器本身失效会让「零命中」变成一句没有依据的话，因此自检进源码门。

CI 定义见 `.github/workflows/ci.yml`。

四个 workflow job 的显示名即 `main` 保护检查的名称：`Source Gates`、`Dataset Gates`、`Web E2E`、`Security Portable`。名称里不含矩阵或版本，可长期稳定引用。`Security Portable` 这个后缀是有意的：该 job 真实门控的是**协议集成**（`test:protocol`）、**浏览器回环安全**（`test:browser-loopback`，BLS-01..09，真实 Chromium + 真实攻击页面）、**数据源故障遏制**（`-Suite FaultContainment`）、**桌面 bundle 生命周期**（`-Suite Desktop`）与**作业对象 fail closed**（`-Suite JobObject`），都不是完整 S1–S11 局域网矩阵。S9（不受允许来源在连接层被拒绝）需要一个非 RFC1918 的真实对端地址，hosted runner 不具备，脚本因此报 `NOT VERIFIED` 并以非零退出；该矩阵在 job 内仍原样运行（`continue-on-error`，不删断言、不改写结论），完整形态属本地/发布门：本机实测 `FAIL=0 NOT_VERIFIED=0 SKIP=0`。因此 `Security Portable` 通过只声称 portable protocol + browser-loopback + 故障遏制 + 桌面生命周期 + 作业对象判据，不代表完整局域网矩阵。

分支保护自 P4R Batch 6B 起 `enforce_admins = true`：四个 required check 对仓库管理员同样是硬门，管理员直接推送 `main` 会被 GitHub 拒绝。所有改动（含报告本身的修订）都经 feature branch → required checks → PR 合入。

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
:: 云端可执行的数据集门（分次执行；`-File` 传入逗号串会被 ValidateSet 拒绝，
:: 而把 P5 与 CLI 写在一次调用里会被显式拒绝——请求了却不执行是最坏的一类假绿）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\regression.ps1 -Suite P5 -Dataset <数据集目录>
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\regression.ps1 -Suite CLI -Dataset <数据集目录>
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

:: 数据源故障遏制与桌面 bundle 生命周期（都不需要游戏资源，只需要数据集）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\regression.ps1 -Suite FaultContainment -Dataset <数据集目录>
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\regression.ps1 -Suite Desktop -Dataset <数据集目录>

:: telemetry plugin：SDK 取回 + 摘要校验（外部前置；不接入 required CI 的网络信任根）
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\prepare-scs-sdk.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\verify-plugin-artifacts.ps1
```

工具链契约固定在仓库内，不依赖"runner 今天预装了什么"：`rust-toolchain.toml`（Rust 1.96.0 + rustfmt/clippy）、`global.json`（.NET SDK 9.0.x）、`.nvmrc`（Node 24.13.0）。行尾契约见 `.gitattributes`：文本一律 LF 入库，`.bat`/`.cmd` 检出为 CRLF（cmd.exe 对 LF-only 批处理的解析不可靠）。


### Web UI（正式前端）

`tools/ets2nav-web/` 的静态产物不入库，须由锁定版本的依赖构建；clean clone 同样按以下步骤恢复，无需人工复制任何文件：

```bat
cd tools\ets2nav-web
npm ci                 :: 依 package-lock.json 恢复确定版本依赖（maplibre-gl 6.4.1 / pmtiles / qrcode / @playwright/test）
npm run build          :: 生成 dist\（vendor 库 + 页面 + build-manifest.json）
```

产物为 `tools/ets2nav-web/dist/`，即 nav-server 的默认 web root（可用 `--web=` 覆盖），也是桌面 bundle 的 `web/` 来源。
`maplibre-gl` 精确钉在 `6.4.1`（修复 critical advisory GHSA-jrc7-96c5-q579 / CVE-2026-85061，影响 `<= 6.4.0`）。MapLibre 6 只发布 ESM，且打包后库无法自行定位 worker，因此构建额外产出 `vendor/maplibre-gl-worker.js`，由 `app.js` 以 `setWorkerUrl` 显式指向；该文件缺失时地图不会进入 loaded 状态，故它属必需产物。`tools/graph-debugger` 是独立的开发工具，**未**随之升级（其自引用仍固定在受影响范围内的 unpkg 4.7.1，且该工具当前无法运行）——它是既有遗留项，不属于发布运行时。

`map.pmtiles` 与 `fonts/` 属运行期可选资源：缺失时前端降级（无底图 / 跳过 city 文字层），并在 console 输出 INFO/WARN，不静默失败。**它们当前尚未随发布包分发**，因此不得把本产品描述为"完整离线地图"；发布契约见 `docs/validation/p4r-batch6a-2026-09.md` §10（组装脚本已支持 `-MapPmtiles` / `-FontsDir`，并把有无如实记入 `bundle-manifest.json`）。
依赖版本、来源与产物 SHA-256 记录于 `dist/build-manifest.json`；打包产物的整体身份记录于 bundle 的 `bundle-manifest.json`（桌面壳与 sidecar 的 SHA-256、前端与数据集的树摘要、数据集自身的 content fingerprint）。

### Desktop（桌面端，P4R Batch 6A 起的边界）

Desktop 不再是「薄壳 + 用户手工启动 server」。`ets2nav-desktop.exe` 在启动时解析自身所在目录，校验随包 sidecar 的字节身份，以 `--port=0` 拉起 `nav-core-cli.exe`，把 WebView 指到该 sidecar 自己在 `127.0.0.1:<运行时端口>` 上提供的页面：

```
ets2nav-desktop.exe
  └─ spawn <bundle>/nav-core-cli.exe server <dataset> --port=0 --web=<bundle>/web
       └─ 同时提供 UI 与 API/WS
  Tauri WebView ── 加载 http://127.0.0.1:<运行时端口>/
```

页面与 API **同源**，因此 bootstrap / WS / `/api/*` 与普通浏览器走完全相同的路径（跨源模型退出产品）。端口经 sidecar 的 stdout 通道 `[server] port=<n>` 协商，**令牌不经该通道**——它只经回环 `/api/bootstrap` 交付。退出码：`0` 正常；`2` 用法；`20` 打包缺陷（sidecar 缺失/身份不符/前端产物缺失/版本与清单不符）；`21` 数据集；`22` sidecar 启动失败；`23` sidecar 运行期意外退出；`24` 作业对象不可用。退出时 sidecar 被显式终止，且由作业对象保证 Desktop 自身被强杀时也不留孤儿——这条保证是**必需**的：作业对象创建或加入失败时 Desktop 拒绝启动（退出码 24，不建窗口），而不是降级为一个不再成立的承诺。开发者可用 `--allow-no-job-object` 显式降级，此时会留下 WARN。

```bat
:: 组装发布 bundle（release 桌面壳 + sidecar + 前端产物 + 数据集）
powershell -NoProfile -ExecutionPolicy Bypass -File desktop\scripts\assemble-bundle.ps1 ^
    -OutDir <输出目录> -Dataset <数据集目录> -Profile release
:: 独立复核：重新读取磁盘上的每个身份并与清单比对
powershell -NoProfile -ExecutionPolicy Bypass -File desktop\scripts\verify-bundle.ps1 -BundleDir <输出目录>
:: 打包产物生命周期（D-01…D-07）；加 --windowed 则在真实 WebView 中验证
node desktop\tests\desktop-lifecycle.mjs --bundle <输出目录> [--windowed]
```

数据集运行时契约是「随包携带」（`<bundle>/data/europe-v5`），启动时校验结构与清单计数；`--verify-dataset-digest` 会额外重算整棵数据集的树摘要（约 373 MB，默认关闭以不拖慢每次启动），该完整摘要由 `verify-bundle.ps1` 在打包校验中无条件重算。默认只监听回环；需要手机作为第二屏时显式加 `--lan`。

### telemetry plugin（SCS SDK provenance 与可复现构建）

两个插件 DLL（`scs-nav-bridge` / `semaphore-bridge`）由 `telemetry-plugin/*/build.bat` 用 MSVC 构建；SDK 头文件不随仓库分发（`vendor/` 被忽略），取回与摘要校验由 `scripts/prepare-scs-sdk.ps1` 承担：固定官方 URL 与文件名、固定 SHA-256（`c6c1f737…023a`，62794 字节，许可为 MIT，允许再分发）、摘要不符即失败且不落盘任何头文件。插件产物另有机器契约检查 `scripts/verify-plugin-artifacts.ps1`（PE x64 DLL、`.def` 声明的导出齐备、镜像内无开发机绝对路径、与 provenance 清单对应）。构建已加 `/Brepro`，因此从不同绝对路径的三次 clean build 逐字节一致。

**是否把该下载接入 required CI 的信任根尚未决定**，因此本轮只把 provenance 清单的**离线契约**放进 `Portable` 门；取回 SDK 仍是打包/发布时的显式外部前置。

### Release Candidate 打包（P4R Batch 6B）

候选版本号为 `0.7.0-rc.1`，单一事实来源是 `desktop/Cargo.toml` 的 `[package] version`：`nav-core-cli/Cargo.toml`、`desktop/tauri.conf.json`、`tools/ets2nav-web/package.json` 三处必须与它一致，`assemble-bundle.ps1` 在组装时核对（不一致即 PRECONDITION FAILURE），Desktop 在启动时也核对清单声明（不一致即退出码 20）。此前 `bundle-manifest.json` 的 `app_version` 是硬编码的 `0.1.0`，与 Cargo 版本长期漂移；现在这条一致性是机器判据，不只是文档约定。

```bat
:: 组装 staging -> 校验 -> 打包 -> 解压到全新路径 -> 再校验 -> 从解压产物跑生命周期；A/B 两次做可复现性对照
powershell -NoProfile -ExecutionPolicy Bypass -File desktop\scripts\run-release-pipeline.ps1 -Dataset <数据集目录>
```

产物是**可移植 ZIP**（`ETS2Nav-<version>-windows-x64-<profile>.zip`），顶层为单一目录 `ETS2Nav/`，内含两个可执行文件、`bundle-manifest.json`、`LICENSE`、`THIRD_PARTY_NOTICES.txt`、`plugins/`、`data/europe-v5/`、`web/`。`profile` 是**测量值**：只有数据集、底图、字形三类资源齐备才写 `FULL`，否则写 `CORE`；因此当前产物是 **Core RC**，不得称 Full Offline。发布层另有 `SHA256SUMS.txt` 与 `release-manifest.json`，后者记录 artifact 字节数/SHA、解压后 bundle 树摘要、两个可执行文件与插件 DLL 的 SHA、数据集与 MapLibre 版本、SCS SDK SHA 以及**实机验证状态**。

第三方许可清单由 `scripts/generate-third-party-notices.ps1` 生成（`-Check` 校验是否过期），所有许可事实取自锁文件、已安装包的 `package.json` 与 `LICENSE`、`cargo metadata` 与 SDK 归档内的许可文本，不手写猜测。Windows 代码签名状态为 **未签名且已披露**——本项目没有代码签名证书，产物不得被描述为「已签名」或「可信发布者」。

**当前不得发布 stable：** B1–B6 实机验收未执行，重建后的插件 DLL 未在游戏内验证，底图与字形未随包分发。候选二进制身份（各 SHA-256）在 `docs/validation/p4r-batch6b-2026-09.md` 中冻结，B1–B6 必须绑定该组 SHA；任一二进制在测试中途改变，对应证据即失效。

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

P4R 批次的逐轮证据见 `docs/validation/` 下的 `p4r-batch*.md`；最近一轮为 `p4r-batch6a-2026-09.md`（发布关键工程：advisory 关闭、故障遏制、桌面产品边界、SDK provenance 与可复现构建），其中如实列出该轮的 local / remote / 未验证项。更早的 `p4r-batch5-2026-09.md` 正文记录的是当轮的测量值（例如当时插件 DLL 的字节大小），**不改写历史正文**，其变更以 `p4r-batch55-2026-09.md` 与 `p4r-batch6a-2026-09.md` 的追补说明为准。

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
