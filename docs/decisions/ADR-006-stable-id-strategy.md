# ADR-006 — Stable ID strategy

**状态**：Accepted（2026-08-10，P1-00）
**依据**：P1 计划 §06

## 背景

见 P1-map-compiler-plan.md 对应章节（依据字段）。

## 决策

内部连续 ID 确定性生成：source UID 直接作为稳定语义 ID；内部索引按输入顺序分配（sector 输入顺序稳定：--all-sectors/--region 按 sector 名排序，显式列表靠参数序），禁止依赖 Dictionary 枚举序/线程调度/随机序。修订 2026-08-10：原定"UID 稳定排序分配"未实现（依赖输入顺序即可达确定性，有回归验证）；semanticHash 概念无实现载体（manifest generated_at 不入对比哈希）

## 后果

- 正向：格式复杂度封装于 P1；P2 只读 Navigation Dataset。
- 反向：schema/接口冻结需在语义图稳定后执行。

## 关联

Gate G6 / P1 风险表。
