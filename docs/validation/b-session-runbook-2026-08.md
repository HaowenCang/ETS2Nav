# B 侧实机会话 Runbook（2026-08-12）

**目的**：一次会话采集 B1+B2+B3 所需的全部数据，关闭 P2 G15 等跨阶段实机遗留。
**预计**：B1 冒烟 5 min + B2 采集 20–30 min + B3 实验 15–30 min ≈ **1 小时**。

---

## 零、前置核对（会话前 5 分钟，必做）

### 0.1 插件就位（已核实，无需操作）

| 文件 | 状态 |
|---|---|
| `scs-nav-bridge.dll` | ✅ 已安装于 `bin\win_x64\plugins\`，SHA-256 与仓库构建产物一致（139776 B） |
| `semaphore-bridge.dll` | ✅ 同上（139264 B） |
| `ets2la_plugin.dll` | ✅ 共存（信号灯数组激活依赖，D5 决策） |

### 0.2 **mod 激活集核对（本次新增，重要）**

`europe-v5` 数据集由 **base + 官方 DLC** 构建，**不含任何 mod**。若会话时激活了改写地图的
mod（本机装有 ProMods 全量 11.2 GB，其中 `promods-eu-map-v281.scs` 含 `/map`），地图数据
将与数据集不一致，T1/T2/T3/T4 的测量结果全部失效。

依据最近一次游戏日志（2026-09-02 15:58）：

```
[mods] Active 17 mods (local: 0, workshop: 17)
promods-*.scs: Unmounted          ← ProMods 未激活
```

即：**本地 mod 全未激活（ProMods 安全）**，仅有 17 个 Workshop mod 生效。其中两个与本次
测量对象直接相关，建议处理：

| Workshop mod | 影响 | 建议 |
|---|---|---|
| Different lenses of traffic lights | 红绿灯灯罩外观 | 可保留（不影响相位） |
| **Flashing Green (Traffic Lights)** | **绿灯闪烁行为** | **建议禁用**——本项目最高风险功能是红绿灯倒计时（±1 s）与 GLOSA，该 mod 会改变灯态表现，可能污染 T3 与 B3 判定 |
| Real Traffic Density ETS2 | 交通密度 | 可保留（不影响相位） |
| Actual Day-/Nighttime Mod / Realistic Brutal Graphics | 光照天气 | 可保留 |
| tree_improved_4k / Beautiful Road Textures | 贴图 | 可保留 |

**会话时须重新核对**（激活集可能已变）：启动游戏后查
`game.log.txt` 的 `[mods] Active` 行，以及是否出现 `local:` 非 0（出现即表示本地 mod 已激活）。
若 ProMods 被激活，**先停用它再采集**，否则数据不可用。

### 0.3 数据集一致性（可选，10 秒）

```
E:\Projects\Pi\ETS2Nav\tools\map-inspector\MapInspector\bin\Release\net9.0\map-inspector.exe ^
  --check-fingerprint --install "E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2" ^
  --dataset "E:\Projects\Pi\ETS2Nav\data\europe-v5"
```

期望 `FINGERPRINT MATCH`（exit 0）。若报 `MODS-FINGERPRINT CHANGED`，说明 mod 集合自数据集
构建后已变——按上表处置后再判断是否需要重建数据集。

---

## 一、B1 冒烟（5 分钟）

1. 启动 ETS2（进入任意存档，菜单内即可）
2. 另开终端：

```
cd E:\Projects\Pi\ETS2Nav\nav-core
target\release\nav-core-cli.exe live
```

3. **判据**：
   - 首行输出 `录制 trace: C:\Users\<你>\AppData\Local\Temp\ets2nav-live-<日期>-<时刻>.navtrace`
   - 随后输出 `等待遥测桥（Local\ETS2NavTelemetry）……`
   - 接着**持续**输出 `TELEMETRY seq=… pos=… speed=… limit=…` 帧（而非一直停在「等待」）
   - 每 60 秒追加一行 `[rec] 已录制 N 帧 → <路径>`，据此确认录制在推进
4. **回传**：确认帧流正常即可（如有异常，贴出错误文本）。

> **trace 路径可省略，也可指定**：`nav-core-cli live` 自动录到 `%TEMP%\ets2nav-live-<时间戳>.navtrace`；
> 若写 `nav-core-cli live D:\my-trace.navtrace` 则录到指定位置。**无论哪种，路径都会在启动时打印**——
> 请记下它，B2 结束时需要用它确认采集产物。
>
> 录制为**逐帧即时落盘**（无用户态缓冲），因此用 `Ctrl+C` 中断不会丢失已写入的帧，无需「正常退出」操作。

---

## 二、B2 综合驾驶采集（20–30 分钟，覆盖 T1–T6）

### 路线（单趟覆盖，尽量依次经过）

```
城市路段（30–50 km/h 区）            → T1 城市限速 + T3 信号路口
    ↓
国道 / 高速（70–90 / 不限速）        → T1 高速限速 + T5 巡航
    ↓
德国不限速高速段（若有）              → T1 第 4 项
    ↓
英国（伦敦/曼彻斯特一带）环岛 ×1–2    → T2 UK 环岛绕行方向
    ↓
设置导航目的地，沿导航行驶 3–5 分钟    → T4 G15 状态机
    ↓
故意偏航一次（驶离导航路线）           → T4 重规划链路
    ↓
到达目的地                            → T4 Arrived
```

### 操作要点

- **两个终端并开**（各自独立，互不干扰）：

  | 终端 | 命令 | 作用 |
  |---|---|---|
  | A | `cd E:\Projects\Pi\ETS2Nav\nav-core` 然后 `target\release\nav-core-cli.exe live` | 录制 trace + 信号灯旁路记录 |
  | B | `target\release\nav-core-cli.exe server ..\data\europe-v5` | 实时 UI（浏览器开 `http://127.0.0.1:8123`） |

  终端 A 启动后先记下首行打印的 trace 路径（B2 结束时要用）。`live` 同时会在同目录生成
  `ets2nav-live-<时间戳>.sem.csv`（信号灯状态记录，供 T3 离线判定）——该文件在检测到
  信号灯共享内存后才创建，因此只要经过有灯路口就会出现。

- 导航目的地：在终端 B 的 UI 上设置（推荐，可直接观察状态 chip）；或用 CLI
  `target\release\nav-core-cli.exe dest <地点名> ..\data\europe-v5` 查出坐标。任选其一，记录用了哪个。
- 偏航后观察 UI 状态 chip 是否走 `SuspectedOffRoute → Rerouting → Navigating`
- **T3 需要停车等待**：经过红绿灯路口时，在停止线前停住等**一整个周期**（红→绿→红），
  让倒计时推进被完整记录；有条件的话做 1–2 个路口。
- **T5 需要两轮**：同一路段分别记录「插件加载」与「插件卸载」的 FPS。
  两轮可在同次会话先后做（卸载插件需移出 DLL 并重启游戏，见下方说明），也可另约。
- T6 复用本轮 trace，无需额外驾驶

### T5 两轮的具体做法

| 轮次 | 插件目录状态 | 导航核心 | 记录 |
|---|---|---|---|
| 基线轮 | `scs-nav-bridge.dll`、`semaphore-bridge.dll` **移出** `bin\win_x64\plugins\`（`ets2la_plugin.dll` 保留） | 不启动 | 该路段 avg / 1% low FPS |
| 负载轮 | DLL **放回** | `nav-core-cli live` + `server` UI 全开 | 同路段 avg / 1% low FPS |

DLL 移出后需**重启游戏**（插件在进程启动时加载）。两轮请跑**同一路段、同一车速区间**，
否则数据不可比。若时间紧张，可退化为「导航开 vs 关」并注明口径——但那测的是核心进程开销，
不等于 P0-D 原始的「ETS2 only vs ETS2+Core」，报告里会分开陈述。

### 交付

| 项 | 说明 |
|---|---|
| trace 文件路径 | `live` 启动时打印；停止后确认文件存在（通常 <10 MB）并给出完整路径 |
| **信号旁路记录** | 与 trace 同目录的 `ets2nav-live-<时间戳>.sem.csv`（T3 判定依据） |
| FPS 记录 | 两轮的 avg / 1% low（游戏内计数器或 Afterburner 均可） |
| 导航目的地 | 用了哪个坐标/POI |
| 会话备注 | 是否经过环岛、是否停车等灯、有无异常

---

## 三、B3 B7 特殊行为实验（15–30 分钟，可与 B2 同一会话顺带）

工具：`tools\signal-lab\SignalLab\bin\Release\net9.0\SignalLab.exe`（采集）
分析与判定证据：`SignalLabAnalyze.exe`；判定结论由我离线出（工具只产出证据）。

### TL-03 Warp（快速旅行 / 休息跳过）

1. 接近信号灯路口，按 `S` 开始会话
2. 执行**快速旅行或休息跳过**（sim 时钟前进跳变）——自动检出，无需按键
   - 若为**位置传送**（goto 类，sim 不变）则不会自动检出，按 `R` 手动标记
3. 观察跳变后相位行为；按 `S` 结束会话

### TL-04 Reset（读档）

1. 会话中在信号路口**读档**（sim 倒退）——自动检出；可按 `R` 确认标记
2. 注意：快速旅行使 sim **前进**（属 TL-03）；读档才触发**倒退**

### TL-05 特殊 profile 路口

1. 经过特殊 profile 路口（`sleep_time` 夜间闪烁 / `blockable` 等）时按 `P` 标记
2. 记录路口大致位置

### 交付

samples.csv 与 events.csv 两个路径（`SignalLab` 输出目录）。

> **若两个信号灯相关 Workshop mod 未禁用**，请在交付时注明——我会在报告中登记该条件，
> 并据此判断相位证据是否可用。

---

## 四、B 侧完成后我做（离线，不需要你参与）

| 项 | 输入 | 产出 |
|---|---|---|
| T1 限速一致率对照 | trace | 闭合 P3 提醒模块启用边界（当前默认关闭交付） |
| T2 UK 环岛方向 | trace | 补充 p2-04 验证记录 |
| T3 信号 runtime 关联置信度 | trace + `*.sem.csv` | 判定是否达 VERIFIED（S≥0.8 且 id 匹配） |
| T4 G15 状态机全程 | trace（`session` 回放） | **关闭 P2 最大遗留** |
| T6 matcher 权重校准 | trace（`match` 回放） | 冻结建议（合成 trace 修复后 HIGH 97.3%） |
| B3 warp/reset 相位判定 | `samples_*.csv` + `events_*.csv` | TL-03/04/05 行为模型结论 |
| 汇总报告 | — | `docs/validation/p3-gameplay-2026-08.md` + 各 closeout 已知限制销账 |

之后进入 B4（提醒模块实机验收，分项 5–10 min）、B5（移动端 LAN，约 10 min）、B6（§63 性能验收，约 15 min）。

---

## 五、已知会影响的测量条件（登记，供会话时留意）

1. **traffic-light Workshop mod**：见 §0.2——建议禁用 `Flashing Green`。
2. **数据集不含 mod**：若激活任何改写 `/map` 的 mod，全部测量失效。
3. **T5 需两轮**：单轮无法给出回退幅度；两轮需同路段同车速区间。
4. **合成 trace 的终点偏移 352 m**（虚拟终点以边终点替代）——仅影响合成数据，不影响你的实机 trace。
5. **live 模式无 20 Hz 节流**（已知遗留）：实时模式直通，可能看到高于 20 Hz 的帧率；不影响采集正确性。
6. **T3 依赖信号旁路记录**：trace 本身不含信号灯字段（`TelemetrySnapshot` 无灯态），
   故信号的离线判定由 `live` 并行写出的 `*.sem.csv` 承担。**若该文件未生成，说明整个
   会话未读到信号灯共享内存**——此时 T3 只能依据 UI 的实时显示做定性结论，请当场留意 UI 卡片是否正常。
7. **B3 与 B2 同会话时**，`SignalLab` 与 `nav-core-cli live` 可同时运行（二者读同一共享内存的
   不同区域，互不写入），但两个程序的按键/输出在不同终端，操作时注意焦点。

---

## 六、命令速查（照抄即可）

```bat
:: 前置：进入仓库根目录
cd /d E:\Projects\Pi\ETS2Nav

:: ① 数据集指纹核对（可选，10 秒）——期望 FINGERPRINT MATCH
tools\map-inspector\MapInspector\bin\Release\net9.0\map-inspector.exe ^
  --check-fingerprint --install "E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2" ^
  --dataset "E:\Projects\Pi\ETS2Nav\data\europe-v5"

:: ② 启动游戏后——终端 A：录制（trace + sem.csv），路径在首行打印
cd nav-core
target\release\nav-core-cli.exe live

:: ③ 终端 B：实时 UI → 浏览器打开 http://127.0.0.1:8123
target\release\nav-core-cli.exe server ..\data\europe-v5

:: ④ B3 实验（第三个终端，可与 ②③ 同时）
tools\signal-lab\SignalLab\bin\Release\net9.0\SignalLab.exe
::    按键：S 开始/结束会话  1=红→绿  2=绿→红  3=其他  R=疑似重置/快速旅行  P=特殊 profile  Q=退出

:: ⑤ 若需在 UI 上设目的地，可先用 CLI 查坐标
target\release\nav-core-cli.exe dest <地点名> ..\data\europe-v5
```
