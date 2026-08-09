# ADR-004 — Dataset format

**状态**：Accepted（2026-08-10，P1-00）
**依据**：P1 计划 §04

## 背景

（P1 计划对应章节定义）

## 决策

routing.graph/junction.graph 用自定义紧凑二进制（magic/version/endianness/offset table，读取方校验）；map.db/search.db 用 SQLite（FTS5）；禁止 .NET BinaryFormatter/JSON 对象树作为正式格式；schema 在语义图稳定后冻结（v1），之前用 experimental schema

## 后果

- 正向：格式复杂度封装于 P1；P2 只读 Navigation Dataset。
- 反向：schema/接口冻结需在语义图稳定后执行。

## 关联

Gate G4 / P1 风险表。
