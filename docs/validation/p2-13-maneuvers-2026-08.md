# P2-13 Maneuver Generator 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §94-99（类型/movement 优先/几何细化/抑制/Keep）。
**产物**：nav-router maneuver 模块 + CLI route maneuver 序列输出。

## 一、实现

### 类型（§95 V1 全集）
Depart/Continue/SlightLeft/Left/SharpLeft/SlightRight/Right/SharpRight/KeepLeft/KeepRight/UTurn/EnterMotorway/ExitMotorway/Roundabout/Ferry/Train/Arrive

### Movement TurnType 优先（§96）
- junction turn 查询表 `(junction_uid, movement_id) → TurnType`（join：routing 边 source_uid + movement_id ↔ junction.graph）
- TurnType 语义（P1 实证）：-1 左 / 0 直 / 1 右 / 2 U

### 几何细化（§97）
- movement 用**真实 polyline entry/exit tangent** 计算绕行角 → slight（<40°）/ normal（40-92°）/ sharp（>92°）
- road 边用 entry/exit tangent 转角

### 抑制（§98）
- 转角 < 25° 且无 movement 转向语义 → 不生成（自然弯曲合并 Continue）——**Europe 实测 80 边 → 39 条（51% 抑制）**

### Keep Left/Right（§99）
- 分叉判定：节点多个 road 出边 + 存在更直的出边 + route 偏转 25-52° → Keep（按偏转方向）

### Enter/ExitMotorway
- road_class 3 边界（class 3 = motorway，P2-08 语义）

### Ferry/Train（§105/106 预置）
- leg 切换 maneuver + 距上距离重置

## 二、验证

### 单元测试（2 个，总计 27 全绿）
- 右转 90° + EnterMotorway + Arrive 序列
- 轻微弯曲（5.7°）完全抑制（仅 Depart/Arrive）

### Europe v4 真实数据（Berlin 4.96km / 80 边）
```
Depart → Left(-59°) → Right(+63°) → SlightRight → SharpLeft(-117°)
→ EnterMotorway/ExitMotorway 交替（城区高速匝道真实结构）
→ SlightLeft 序列 → ... → Arrive
```
39 条 maneuver——转向/高速/抑制逻辑协同工作

## 三、门

- fmt / clippy 0 / 27 测试全绿
- 细化阈值（slight/normal/sharp）为 corpus 校准前默认（P2-14 环岛/语料验证后冻结）

## 四、下一步

- P2-14 Roundabout/Transit：环岛拓扑检测（§101）+ 出口编号（§102）+ Ferry/Train leg 正式化（§105/106）
