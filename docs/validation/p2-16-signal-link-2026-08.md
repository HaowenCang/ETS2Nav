# P2-16 Signal Linker 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §111-117（桥接/静态几何/runtime 关联/置信度/边界）。
**产物**：nav-router signal 模块 + nav-telemetry semaphore 共享内存读取 + CLI `signal` 命令。

## 一、实现

### 静态绑定（§111/112）
- `next_controlled_movement`：route 中下一个受控 movement（`SemaphoreId >= 0`）→ (route 边索引, movement_edge_id, junction_uid, semaphore_group)
- **不推断 major/minor**（§116——只保证 movement ↔ 灯组对应）

### 静态 signal head 姿态（§113 近似）
- dataset v2 未存灯头几何（V2-6 推迟项）——用 **movement polyline 入口端 + 入口 tangent** 近似

### Runtime 关联（§114 评分）
```
S = 0.5·S_pos + 0.3·S_heading + 0.2·S_id
S_pos：静态 head 与灯位置距离（15m 内线性衰减——只匹配附近灯）
S_heading：head 方向 vs 灯 quat yaw（绕 Y 提取公式）
S_id：灯 id == semaphore_group → 1
```
- VERIFIED（S≥0.8 且 id 匹配）/ PROBABLE（S≥0.55）/ UNKNOWN（§115——只有 VERIFIED 可交 P3）

### Runtime 数据源
- nav-telemetry `read_semaphores()`：Local\ETS2NavSemaphore（semaphore-bridge v8：16B header + 48B×N 灯槽）
- 无游戏/桥接器 → graceful None

## 二、验证

### 单元测试（4 个，总计 32 全绿）
- 受控 movement 查找（junction uid / group 精确）
- id+位置匹配 → VERIFIED + Red 状态 + 倒计时
- 位置/方向近但 id 不匹配 → PROBABLE
- 远处灯 → UNKNOWN（不关联）

### Europe v4 真实数据（Berlin 4.96km 路线）
```
受控 movement @route边8（junction=0x355a… group=3）→ Unknown
受控 movement @route边72（junction=0x341d… group=0）→ Unknown
```
- 静态绑定正确（信号路口在路线中识别）；runtime 关联在 CLI 无游戏环境正确 Unknown

### 修复记录
- light_yaw 公式轴错误（绕 Y 提取：atan2(2(wy-xz), 1-2(y²+z²))）
- 字节字符串转义（python 写文件反复失败——最终 write 工具重写）

## 三、门

- fmt / clippy 0 / 49 测试全绿（workspace 总数）
- runtime 关联的**真实游戏验证**（§115 VERIFIED 实测）记 pending（游戏会话排除）

## 四、下一步

- P2-17 Navigation session：全模块协调状态机（Idle/Planning/Navigating/OffRoute/Rerouting/Arrived，§118-121）+ NavigationSnapshot 统一输出
