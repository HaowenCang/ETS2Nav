# ADR-007 — POI access model

**状态**：Accepted（2026-08-10，P1-00）
**依据**：P1 计划 §07

## 背景

（P1 计划对应章节定义）

## 决策

POI 区分 visual position 与 routing access node；需要导航的 POI（company/fuel/rest/ferry/train）必须具有合法 access node，否则明确标为非 routing POI

## 后果

- 正向：格式复杂度封装于 P1；P2 只读 Navigation Dataset。
- 反向：schema/接口冻结需在语义图稳定后执行。

## 关联

Gate G7 / P1 风险表。
