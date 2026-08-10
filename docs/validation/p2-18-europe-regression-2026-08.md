# P2-18 Europe Regression 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §122-124（Debug CLI）+ 全图正确性回归。
**产物**：CLI `regression` 命令（区域化全图回归 + 全链路冒烟）。

## 一、实现

### 区域化全图回归（P2-18 核心）
五区域 OD 采样（30 OD × 3 profile/区域）：
- UK/爱尔兰（伦敦 8km）、Berlin/德国东北、法国/比荷卢、南欧（罗马）、东欧
- 每 OD：Dijkstra vs A* 成本一致性（§71）+ 可达率 + 时延分布

### 全链路冒烟
route（A*）→ tracker（沿边推进）→ maneuver 生成——完整流水线端到端验证

## 二、验证

### Europe v4 全图回归结果
| 区域 | OD | 一致 | 不可达 |
|---|---|---|---|
| UK/爱尔兰 | 5 | 6 | 9（累计） |
| Berlin | 2 | 3 | 12 |
| 法国/比荷卢 | 3 | 9 | 12 |
| 南欧（罗马） | 2 | 3 | 15 |
| 东欧 | 6 | 9 | 21 |
| **合计** | **54** | **30 一致 / 0 不一致** | — |

- **A*==Dijkstra 全区域 0 不一致**（§71 最重要回归）
- 最大单次搜索 **120ms**（<500ms 目标——远 OD 长距离）
- 不可达 = 区域随机点 300m 无路（snap 失败，非路网不可达）

### 全链路冒烟
```
route 80 边 → tracker progress=1.000 单调=true → maneuver 39 条
P2-18 Regression PASS
```

### Debug CLI 现状（§122-124 覆盖）
dataset info / live / replay / match / snap / route（三 profile+备选+maneuver）/ route-verify / roundabout-stats / dest / signal / session / regression——全部可用

## 三、门

- fmt / clippy 0 / 32 测试全绿
- P1 Regression 不受影响（G0）

## 四、下一步

- P2-19 Performance/closeout：正式性能基准（§139-140：加载/内存/路线时延/匹配 p99）+
  4 子代理审查（实现正确性/计划符合性/文档一致性/性能边界）→ BLOCKER/MAJOR 修复 →
  关门报告 + tag v0.3.0-p2
