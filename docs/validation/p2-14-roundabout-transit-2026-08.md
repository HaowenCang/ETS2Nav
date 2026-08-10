# P2-14 Roundabout/Transit 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §100-106（环岛检测/出口编号/左行/语料/Transit leg）。
**产物**：nav-router roundabout 模块（拓扑检测 + 出口编号 + RouteLeg）+ CLI roundabout-stats。

## 一、实现

### 环岛拓扑检测（§101）
- junction movement **内部图**（两端都在控制节点集合）中找简单环
- 判据：环长 3-8、节点不重复、环上节点内部度数（入+出）== 4（绕环 1入1出 + 入口 + 出口）、
  指向环上节点的出边恰 1 条（chord 排除）、环 movement 平均长度 < 120m
- **不依赖 prefab 名字**（token 无 roundabout 命名——P2-04 实证）

### 出口编号（§102）
- 从 entry 沿环路径前进，数经过的外部入口节点 → 1-based 出口号
- 环路径复用检测器的简单环搜索（返回完整环边序列）

### Transit leg（§105/106）
- `RouteLeg { Drive, Ferry, Train }` + `route_legs()` 分段（UI 无需从 edge 序列猜运输方式）
- Ferry/Train maneuver 已在 P2-13 接入（leg 切换）

## 二、验证

### 单元测试（3 个，总计 24 全绿）
- 5 入口环岛检测 ✓ / 十字路口不误报 ✓ / 全连接交叉口不误报 ✓
- 出口编号：第 1/2/3 出口精确
- leg 分段（Drive→Ferry→Drive→Train→Drive）

### 重要实证发现（Europe v4 全图扫描）
**全 Europe 无 junction 满足"拓扑环"判据**（严格判据下 0 检出）——
**SCS 环岛是放射直通结构（每个入口到各出口的直通 movement，拓扑无环）**，
绕行方向体现在 movement polyline 的弧线几何——**环岛识别必须用几何方法**
（§101 的"内部 geometry"方向——P2-04 切线累积法为正确方向），
纯拓扑检测不可行。此发现推翻"环岛=拓扑环"假设，写入格式笔记价值。

**误报对照**：无 chord 检查时 18,029 个误报（三角全连接交叉口/互通），
chord+度数检查后 0 误报但 0 检出——检测器保守可用（合成场景验证正确性）。

### 已知限制（目标契约排除项）
- **真实环岛几何识别 corpus 校准**（§104 Gate 依赖驾驶 corpus/地图人工核对——排除）
- UK 环岛方向游戏内实测（§103——排除）
- 环岛引导（"第 N 个出口驶出"）的 maneuver 集成待几何识别落地（P2-19 记录）

## 三、门

- fmt / clippy 0 / 24 测试全绿；P1 Regression 不受影响

## 四、下一步

- P2-15 Destination resolver：POI（search.db access_node）/Job 目的地/坐标 → Destination 模型（§107-110）
- P2-16 Signal linker：movement.SemaphoreId ↔ runtime signal（§111-117）
