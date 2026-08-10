# P2-06 Map Matching 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §45-53（候选评分/heading/rolling tracker/置信度/验证）。
**产物**：nav-matcher crate + CLI `match` 命令（trace 回放匹配）。

## 一、实现

### Rolling candidate tracker（§49 局部 Viterbi）
- 每帧：spatial 半径查询 → 逐边评分 → Top-K（默认 8）→ 最优
- 拓扑连续性：上一帧边 to == 候选 from（续行 1.0）/ 同节点折返 0.3 / 断链 0.0
- 置信度（§51）：HIGH（lateral<8m）/ MEDIUM（<20m）/ LOW / UNMATCHED

### 候选评分（§47）
```
S = 0.5·Sd + 0.3·Sh + 0.2·St
Sd：横向距离分（30m 尺度衰减）
Sh：航向分——投影点 tangent（§48 非端点方向）；双向路取 min(d, π-d)
St：拓扑连续性
```

### 自适应半径（§44）
- 已锁定：60m 小半径；未锁定/丢失 >30 帧：300m 大半径重新捕获

## 二、验证

### 单元测试（3 个，总计 14 全绿）
- 直路 50 帧锁定稳定（噪声 ±2m 不跳边）
- **平行路不跳变**（A/B 相距 100m，lateral 2m 时 40 帧稳定 A）
- U-turn 拓扑跟随（折返后经连接边转向）

### 真实 trace 回放（P0 2026-08-09 Berlin 实驾 517 帧）
| 指标 | 实测 |
|---|---|
| HIGH / MEDIUM / LOW / UNMATCHED | **250 / 196 / 71 / 0**（HIGH+MED 86.3%） |
| 匹配帧横向距离均值 | 7.0 m |
| 锁定边序列 | 连续（116245→116246 相邻边） |

0 未匹配、边序列连续——**匹配器基本可用**。LOW 帧主因：trace 转换时 quat 为占位
（yaw 提取误差）→ 航向分偏低；真实运行时 quat 精确，预期 HIGH 占比更高。

## 三、配置与调参

- 权重/阈值在 MatcherConfig（trace calibration 后冻结——计划 §47/§53）
- route bias（§50）预留接口未接（P2-12 rerouting 时启用）

## 四、下一步

- P2-07 Snap model（起终点吸附：matched edge + offset → 虚拟起终点）
- P2-08 Cost profiles（fastest/shortest/balanced）
- P2-09 Dijkstra + A*（route search 正式化）
