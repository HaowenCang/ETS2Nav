# P2-08 Cost Profiles 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §58-69（EdgeCostProvider/三 profile/信号/转弯/GpsAvoid/Secret）。
**产物**：nav-router cost 模块。

## 一、实现

### EdgeCostProvider（§58）
`EdgeCostProvider { profile, params }`——route search 与策略解耦；统一 nonnegative cost。

### 三 profile（§69）
| profile | 成本 | 依据 |
|---|---|---|
| Fastest | 时间：L/v + 信号延迟 + 转弯 penalty | §60/63/64 |
| Shortest | 距离 L | §59 |
| Balanced | 0.7·T + 0.3·D(km) | §68（权重 corpus 校准前默认） |

### 速度模型（§60-62）
- Road：限速 → v；**未知（-1）→ fallback 50km/h + 标记 estimated**（§61）；**无限（0）→ cap 130km/h**（§62）
- Movement：v_junction = 30km/h
- Ferry/Train：30km/h 占位（P2-14 细化）
- road_class fallback：3=motorway 90 / 2=express 70 / 1=local 40 / 0=其他 50

### 静态信号延迟（§63）
`SemaphoreId >= 0` 的 movement → +8s 统计延迟（可配置；不做实时 phase 预测——P3 边界）

### 转弯 penalty（§64）
movement 真实 polyline entry/exit tangent 夹角：急转（≥69°）+3s、U-turn（≥149°）+30s

### 可用性（§65-67）
- GpsAvoid：+600s 高 penalty（非绝对不可通行）
- Secret：+3600s 强 penalty（corpus 验证前默认强排除倾向）
- NoAiVehicles：仅 metadata（不做禁止推断）

## 二、验证

- **4 单元测试**（总计 7 全绿）：fastest 时间成本（1000m@100km/h=36s）/ shortest 距离 / 受控 movement 静态延迟 >8s / 未知限速 fallback+标记+无限速 cap
- clippy 0 warnings / fmt 通过

## 三、门

P2-08 完成——纯函数模型，真实数据全链路验证随 P2-09 route 命令进行。

## 四、下一步

P2-09 Dijkstra Oracle + A*：generation counter 预分配、binary heap、A*==Dijkstra 回归（§70-78）、CLI route 命令（§124）
