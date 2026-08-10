# P2-10 Alternatives 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §79-82（三策略/去重/生成/质量）。
**产物**：nav-router alternatives 模块 + CLI route 备选输出。

## 一、实现

### 流程（§79-82）
1. **三 profile 各搜一次**（§79：FASTEST/SHORTEST/BALANCED）
2. **overlap 去重**（§80）：`O(A,B) = 共享长度 / min(LA, LB)`，阈值 0.85（corpus 调整前默认）
3. **独立路线 <2 条 → penalty 重搜**（§81）：最优路线每边加 `2×边成本` 的 overlap penalty（RouteRequest 新增 edge_penalties 注入）→ 同 profile 重跑 A*
4. **质量检查**（§82）：候选成本 > 2× 最优拒绝；成本升序；截断 3 条

## 二、验证

### 单元测试（2 个，总计 20 全绿）
- 井字形图：≥2 条独立路线且两两 overlap < 0.85
- 成本升序

### Europe v4 真实数据
| 路线 | 结果 |
|---|---|
| Berlin 城区 11.3km | 1 条（fastest/shortest overlap >0.85 正确去重；penalty 重搜无独立绕行——城市单路径合理） |
| Berlin→北向 | 1 条（单路径） |
| **London 城区** | **2 条独立路线：3.7km（fastest）+ 6.0km（penalty 重搜绕行，overlap <0.85）** |

### RouteRequest 扩展
- `edge_penalties: Vec<f64>`（每边额外成本）+ `with_penalties` 构造器——搜索核心零侵入

## 三、门

- fmt / clippy 0 / 20 测试全绿
- P1 Regression 不受影响

## 四、下一步

- P2-11 Route tracker：route progress/distance/ETA（suffix 预计算 O(1)，§83-87）+ 单调性防跳变
- P2-12 Rerouting：偏航证据 + 状态机（ON_ROUTE/SUSPECTED/OFF_ROUTE/REROUTING）+ 1s 重规划
