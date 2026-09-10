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

3. **判据**：输出**持续**遥测帧（位置/速度/时间戳变化），而非「无法连接」。
   同时会打印 trace 录制路径（默认 Temp）。
4. **回传**：确认帧流正常即可（如有异常，贴出错误文本）。

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

- 全程保持 `nav-core-cli live` 录制
- 导航目的地：可用 UI（`server` 子命令）或用 CLI `dest`；任选其一，记录用了哪个
- 偏航后观察 UI 状态 chip 是否走 `SuspectedOffRoute → Rerouting → Navigating`
- **T5 需要两轮**：同一路段分别记录「插件加载」与「插件卸载」（或导航开/关）的 FPS。
  两轮可在同次会话先后做，也可另约。
- T6 复用本轮 trace，无需额外驾驶

### 交付

| 项 | 说明 |
|---|---|
| trace 文件路径 | `live` 启动时打印；确认文件存在并给出路径 |
| FPS 记录 | 两轮的 avg / 1% low（游戏内计数器或 Afterburner 均可） |
| 导航目的地 | 用了哪个坐标/POI |

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

| 项 | 产出 |
|---|---|
| T1 限速一致率对照 | 闭合 P3 提醒模块启用边界（当前默认关闭交付） |
| T2 UK 环岛方向 | 补充 p2-04 验证记录 |
| T3 信号 runtime 关联置信度 | 判定是否达 VERIFIED（S≥0.8 且 id 匹配） |
| T4 G15 状态机全程 | **关闭 P2 最大遗留** |
| T6 matcher 权重校准 | 冻结建议（当前占位口径 HIGH+MEDIUM 86.3%；合成 trace 修复后 HIGH 97.3%） |
| B3 warp/reset 相位判定 | TL-03/04/05 行为模型结论 |
| 汇总报告 | `docs/validation/p3-gameplay-2026-08.md` + 各 closeout 已知限制销账 |

之后进入 B4（提醒模块实机验收，分项 5–10 min）、B5（移动端 LAN，约 10 min）、B6（§63 性能验收，约 15 min）。

---

## 五、已知会影响的测量条件（登记，供会话时留意）

1. **traffic-light Workshop mod**：见 §0.2——建议禁用 `Flashing Green`。
2. **数据集不含 mod**：若激活任何改写 `/map` 的 mod，全部测量失效。
3. **T5 需两轮**：单轮无法给出回退幅度。
4. **合成 trace 的终点偏移 352 m**（虚拟终点以边终点替代）——仅影响合成数据，不影响你的实机 trace。
5. **live 模式无 20 Hz 节流**（已知遗留）：实时模式直通，可能看到高于 20 Hz 的帧率；不影响采集正确性。
