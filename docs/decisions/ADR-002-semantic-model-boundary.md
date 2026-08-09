# ADR-002 — Semantic model boundary

**状态**：Accepted（2026-08-10，P1-00）
**依据**：P1 计划 §02

## 背景

（P1 计划对应章节定义）

## 决策

ScsMapModel 层将 SCS 格式对象转换为 ETS2Nav 导航语义对象；Layer2（格式）与 Layer3（语义）严格分离，格式层不承担业务语义，语义层不接触 .scs/.sii 细节

## 后果

- 正向：格式复杂度封装于 P1；P2 只读 Navigation Dataset。
- 反向：schema/接口冻结需在语义图稳定后执行。

## 关联

Gate G2 / P1 风险表。
