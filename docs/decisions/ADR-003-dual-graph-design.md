# ADR-003 — Dual graph design

**状态**：Accepted（2026-08-10，P1-00）
**依据**：P1 计划 §03

## 背景

见 P1-map-compiler-plan.md 对应章节（依据字段）。

## 决策

Routing Graph（宏观、长距离规划）+ Junction Graph（精细路口/movement/signal）双图并存，通过 junction_id/movement_id 关联；禁止合并为超细粒度单图

## 后果

- 正向：格式复杂度封装于 P1；P2 只读 Navigation Dataset。
- 反向：schema/接口冻结需在语义图稳定后执行。

## 关联

Gate G3 / P1 风险表。
