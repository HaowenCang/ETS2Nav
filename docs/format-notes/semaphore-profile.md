# SCS def 资源与 semaphore profile 格式笔记（A2/B2）

来源：官方 scs_extractor 1.55 解包 `def.scs`（2026-08）。状态：✅ 基础结构已确认（A2 完成；B2 解析器待实现）

## def.scs 解包结构

- `def/world/semaphore_profile.sii` —— **6423 行，278 个 profile**（含 base_share/mod_benelux_reskin/mod_greece/mod_polar_circle 变体文件）
- mod 扩展机制（官方注释）：新增条目用 `<base_name>.<idofyourmod>.sii`，避免冲突

## semaphore_profile.sii 字段语义（官方文件注释 + 结构实证）

| 字段 | 语义 | 证据 |
|---|---|---|
| `interval[]` | `(green, yellow, red, yellow)` 各段秒数；**数量 = 相位组数**（通常 1–2 组，如 2ph 仅 1 条、cr_1x1 有 4 条） | 官方注释 `# green > yellow > red > yellow`，如 `(15.0, 2.0, 23.0, 2.0)` |
| `cycle[]` | **数量 = 信号灯数**（每个灯一个相对周期起点的偏移秒）；与 interval 数量不同属正常 | 2ph: `0.0 / 21.0`（2 灯）；3ph: `0.0 / 21.0 / 42.0`（3 灯） |
| `model[]` / `type[]` | 灯模型后缀与类型（traffic_light_major/minor 等） | 文件中直接可见 |
| `sleep_time_start/end` | 夜间闪烁窗口（自午夜分钟） | 46/278 个 profile 含此字段；官方注释 `1410 # 23:30`、`180 # 03:00`。**不是所有 profile 都有** → 必须逐 profile 解析（v0.2 §34） |
| `inherited` | 继承 fallback profile | 169/278 个 profile 使用 → 解析器必须实现继承链解析与循环检测 |

## 对实现的约束

- B2 解析器：SII 文本解析（`SiiNunit` 块、`tr_semaphore_profile : <unit>`、数组字段、注释剥离、继承链解析）。
- 周期总长 C = 四段 interval 之和；单相位红灯段吸收余量（社区文档）。
- 地图 prefab 引用 profile 名（unit 名如 `tr_sem_prof.cr_1x1`），映射关系在 sector 二进制中（A4 验证）。

## 其他待解包资源

- `base_map.scs`（324 MB）：地图 sector（`map/europe/sec+*.base`），解包进行中
- `base.scs`（10 GB）：prefab 描述（.ppd）、road looks 等；A4 阶段按需解包
