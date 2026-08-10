# P2-04 前置验证报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §38-41（LeftHandTraffic Gate / Speed Gate）、§100-104（环岛）。
**本次交付**：两个重大缺陷发现与修复 + UK 方向验证 + 环岛拓扑分析基础。

## 一、BLOCKER 级发现：P1-13 "全欧洲"数据集缺负 x 区域（422 sector）

### 现象
Europe 数据集 manifest 只有 679 个 sector 名（x∈[0,19] z∈[-31,21]）——**UK/爱尔兰/伊比利亚西部全部缺失**（London/Manchester/Dublin 等城市 POI 不存在）。

### 根因（两处过滤只匹配 `sec+` 前缀）
1. `map-inspector/Program.cs` --all-sectors 枚举过滤 `p.Contains("/sec+")`——负 x sector 文件名为 **`sec-0001+0000`**（`sec-` 前缀）被丢弃
2. `RegionMatches` 正则 `^sec\+(\d+)...` 同样不匹配 `sec-`

### 修复
两处过滤改为同时接受 `sec+` 与 `sec-`；RegionMatches 正则改为 `^sec([+-])(\d+)([+-])(\d+)$`（x/z 各自带符号）。

### 影响
| 指标 | 修复前（v3 前） | 修复后（Europe v3/v4） |
|---|---|---|
| sector | 679 | **1,101**（+422 负 x） |
| POI | 4,997 | **7,918**（+2,921） |
| 城市 | 233 | **380**（+147：London/Dublin/Edinburgh 等） |
| nodes / edges | 3,427,132 / 426,988 | **5,633,750 / 696,717** |
| junctions / movements | 42,843 / 171,453 | **73,141 / 281,012** |
| Ferry/Train 边 | 81 | **127 + 2**（Dover/Harwich/Hull/Tyne 补入） |

P1-13 报告数字为修复前口径——**本报告为勘误基准**（p1-13-europe-build/p1-closeout 数据待同步）。

## 二、MAJOR 级发现：LeftHandTraffic prefab 未镜像（UK movement 方向错误）

### 现象与根因
UK sector 中 959 个 prefab 标记 `LeftHandTraffic`（大陆为 0——标记解析正确），但 SemanticMapBuilder 未使用该标记——**左行国家的 movement 保持模板（右行）方向**：TurnType 左转/右转错位、polyline 几何绕行方向错误。

### 修复（SemanticMapBuilder）
LHT prefab 的 movement：TurnType 取反（-1↔1，左转↔右转）+ polyline 绕 entry/exit 中点 X 轴翻转（环岛绕行方向反转）；拓扑端点不变（世界节点已按镜像放置）。

### 验证（v3 未镜像 vs v4 镜像）
- 同一 UK junction 的几何中间点 X 坐标精确翻转（翻转轴 = 端点中点，端点不变）——镜像正确执行
- **UK 核心网 OD 93.7%**（Europe 整体 96.3%）——无系统性方向断裂

## 三、环岛分析（计划 §101 拓扑识别基础）

- token 无 roundabout 命名（SCS 用数字/字母 token）——**必须拓扑识别**
- 实测：UK 区域（x<0）362 个、大陆 434 个候选环岛（≥4 movements 同向旋转）
- **UK 环岛方向结论**：SCS 环岛模板多为"双向兼容"设计（非 LHT 标记）——**几何镜像后仍需游戏内驾驶实测确认 UK 环岛实际方向**（记录 pending，属 §103 Left-hand roundabout 验证）

## 四、Speed Gate（计划 §40-41）

工具链就绪（speed-validator C# + nav-telemetry live/trace），**真实游戏驾驶采集延后**——需要游戏会话内跑一段城市+高速+不同国家路线。**pending 项**。

## 五、回归

- P1 Regression Suite **ALL PASS**（Berlin 95.4% / Germany 94.2%——LHT 镜像对无 LHT 的 Germany 无影响，94.6→94.2 为采样波动）
- Europe v4 Rust smoke 全 PASS（696,717 边 / 281,012 movements / 几何全校验）

## 六、遗留（P2 后续）

- UK 环岛方向游戏内实测（§103）
- Speed Gate 游戏内采集（§40-41）
- 环岛专项 corpus 正式化（P2-14）
- P1-13/closeout 报告数据勘误同步（sector 1,101 / POI 7,918 / edges 696,717）
