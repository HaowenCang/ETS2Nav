# Telemetry 通道与属性清单（B1 定稿）

来源：官方 SDK 1.14 头文件（vendor/scs_sdk_1_14/include/，2026-08 核对，无需依赖第三方 wrapper）。
状态：✅ 已核对（任务 B1 完成）

## 关键结论

1. **channels（通道）** 中 job 相关仅有 `job.cargo.damage`。
2. **configs（属性系统）** 提供任务目的地：`destination.city`、`destination.city.id`、`destination.company`、`destination.company.id`（及 source 侧）。官方属性名为 `destination.city` 系列；社区/RenCloud 封装中的 `job.city.destination` 等命名是第三方字段名，**不是官方 API 名**。
3. 任务生命周期事件（gameplay events）：`job.cancelled`、`job.delivered`。
4. 道路设施事件（官方事件，可作 Warning Engine 输入）：`player.fined`（闯红灯罚款）、`player.tollgate.paid`、`player.use.ferry`、`player.use.train`。

## 官方 channels（本项目需要）

| 通道名 | 类型 | 语义 |
|---|---|---|
| `truck.placement` | dplacement | 双精度世界坐标 + 四元数（位置/朝向） |
| `truck.speed` | float | m/s，负值=倒车 |
| `truck.navigation.speed.limit` | float | m/s，Route Advisor 限速值；无限速段可能为 0（未文档化，需实测） |
| `truck.fuel.amount` | float | 升 |
| `truck.fuel.range` | float | km |
| `truck.fuel.warning` | bool | 低油量 |
| `truck.fuel.consumption.average` | float | 平均油耗 |
| `truck.fatigue` | — | **不存在于官方 SDK 1.14**（研究 B1 初版有误，RenCloud 亦无此通道）；疲劳相关仅 `rest.stop` 可用 |
| `game.time` | u32 | 游戏内分钟（自首个游戏日 00:00） |
| `local.scale` | float | 真实秒 ↔ 游戏秒倍率 |
| `rest.stop` | s32 | 距下次强制休息的游戏内分钟（实现相关，可能缺失） |

## 官方 configs（job）

| 属性名 | 语义 |
|---|---|
| `destination.city` / `destination.city.id` | 目的地城市显示名 / 内部 ID |
| `destination.company` / `destination.company.id` | 目的地公司显示名 / 内部 ID |
| `source.city(.id)`、`source.company(.id)` | 出发地同构 |
| `cargo`、`cargo.id`、`cargo.mass`、`income`、`delivery.time` 等 | 货物与报酬 |

## 官方 events（frame_start）

`scs_telemetry_frame_start_t` 回调参数（微秒 u64）：
- `render_time`：渲染时间，随帧率变化
- `simulation_time`：物理仿真时间，固定步长；**暂停时仍前进**
- `paused_simulation_time`：仿真时间，**暂停时停止**

官方未规定零点与重置语义（v0.2 §29：load 等情况下可能 restart，不可未验证当作绝对时钟）。

## 对实现的约束（v0.2 §5）

- 最终实现必须以本清单（头文件原文）为准；插件注册 configs 时按 `scs_config` API 处理 `job` 属性组。
- `destination.company.id` 与本地 POI 数据库匹配（v0.2 §51）。
- P0-B 实验需同时记录 Windows monotonic clock 与上述各 clock（v0.2 §29）。
