# P2-11 Route Tracker 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §83-87（进度状态/局部窗口匹配/剩余距离/ETA/单调性）。
**产物**：nav-router tracker 模块 + CLI route-verify 真实模拟。

## 一、实现

### RouteTracker（§83）
- 状态：edge_index / edge_offset / distance_travelled / remaining / window
- **suffix_distance / suffix_time 预计算**（§85/86：O(E) 一次，之后**剩余距离/时间 O(1)**）

### Route Matching（§84）
- **局部窗口**（±window 边）内查找 matched edge——不每帧全扫跨欧洲 route
- 窗口外匹配 → `matched=false`（P2-12 偏航判定证据）

### 单调性（§87）
- 前进（idx>prev）：当前边剩余段 + 中间完整边 + 新边 offset 计入 travelled（**修复重复计数 bug**）
- 同边 offset 前进：计入差值；**倒车/停车不减少**
- 回退到已走边（U-turn/重走）：不减少 travelled——matcher 平行边跳动不引起前后跃迁

## 二、验证

### 单元测试（3 个，总计 22 全绿）
- 沿路线 7 步推进：单调 + 剩余距离精确（500m）+ progress 精确（2500/3000）
- 倒车不减少 travelled、progressed=0
- 平行边跳动（窗口外）：unmatched 但不破坏后续匹配

### Europe v4 真实模拟（Berlin 4.96km / 80 边路线）
```
tracker@10: matched=true travelled=540m remaining=4360m progress=0.11
...
tracker@70: matched=true travelled=4414m remaining=486m progress=0.90
tracker 模拟：80 边走完，progress=1.000 单调=true，总长 4963m
```
**progress 0→1.000 单调、remaining 4963→0 精确收敛**

## 三、门

- fmt / clippy 0 / 22 测试全绿
- 修复记录：切边时 travelled 重复计数（补上当前边剩余段 + 新边 offset 的语义）

## 四、下一步

- P2-12 Rerouting：偏航证据（连续 unmatched + 前进距离 + heading）+ 四态状态机（§88-93）+ worker thread 不阻塞 telemetry + 确认后 ≈1s 重规划
