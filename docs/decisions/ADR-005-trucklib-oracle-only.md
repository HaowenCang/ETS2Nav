# ADR-005 — TruckLib oracle-only

**状态**：Accepted（2026-08-10，P1-00）
**依据**：P1 计划 §05

## 背景

（P1 计划对应章节定义）

## 决策

TruckLib（GPL-2.0）仅作 differential validation oracle，不作 production 依赖；复用任何 GPL 代码须记录来源/许可证/版权并符合 GPL-3.0 及上游许可要求（docs/third-party/）

## 后果

- 正向：格式复杂度封装于 P1；P2 只读 Navigation Dataset。
- 反向：schema/接口冻结需在语义图稳定后执行。

## 关联

Gate G5 / P1 风险表。
