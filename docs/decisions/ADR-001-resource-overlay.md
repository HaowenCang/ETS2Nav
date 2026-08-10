# ADR-001 — Resource Overlay（虚拟 SCS 文件系统与覆盖解析）

**状态**：Accepted（2026-08-10，P1-00）
**依据**：P1 计划 §9–§10

## 背景

P0 直接读 `.scs` archive（HashFsReader）。P1 需要处理 base/def/map archive + 官方 DLC + loose files 的覆盖关系。若覆盖顺序错误，会解析到错误的 prefab/road look/semaphore profile 定义，产生"解析正确但语义错误"的隐性故障。

## 决策

新增 `ScsResource` 层，定义 `IScsResourceProvider { Exists/Open/Enumerate }`，实现 `HashFsProvider`、`DirectoryProvider`、`OverlayProvider`。所有上层模块（Sector/Sii/Definitions/Prefab）只通过虚拟路径读取资源（如 `/map/europe/...`），不感知物理 archive。Overlay 顺序与 ETS2 实际加载逻辑一致（base → DLC 优先级序）。修订 2026-08-10：冲突溯源实现为 `ResolveSource` 单点查询（高优先级胜出者），overridden/effective 冲突日志未实现（无消费者，P2 需要时再补）。

## 后果

- 正向：单一资源入口，DLC/未来 Mod 可插拔；冲突可诊断。
- 反向：需要维护 ETS2 archive 优先级知识；P1-02 是 blocking module。

## 关联

Gate G1（Base + 全部官方 DLC 自动识别）；风险 R4（Definition Override）。
