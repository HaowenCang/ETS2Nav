# ADR-006 — Stable ID strategy

**状态**：Accepted（2026-08-10，P1-00）
**依据**：P1 计划 §06

## 背景

（P1 计划对应章节定义）

## 决策

内部连续 ID 由稳定排序（source UID + source type）确定性生成，禁止依赖 Dictionary 枚举序/线程调度/随机序；buildTimestamp 允许变化但不进入 semanticHash

## 后果

- 正向：格式复杂度封装于 P1；P2 只读 Navigation Dataset。
- 反向：schema/接口冻结需在语义图稳定后执行。

## 关联

Gate G6 / P1 风险表。
