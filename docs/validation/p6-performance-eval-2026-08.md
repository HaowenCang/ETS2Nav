# P6 性能评估报告（A4，2026-08-12；审计修复版）

**目标**：PLAN-P3plus.md §2 A4（P6 性能优化：ALT/CH 分层路由评估 + 增量编译指纹）
**依据**：v0.2 §62（验收标准）、§9（地图更新检测）、§140-142（细化目标）、P1-02（fingerprint 基础）、P2-19/P3-08 bench 数据
**审计修复（2026-08-12）**：目标出处与裕度口径更正（§62 原文 vs §141/142/§140 细化）、指纹验证记录与交付状态对齐、字段名更正、挂接声明删除、§9 覆盖缺口登记。

## 一、ALT/CH 分层路由——评估结论：登记"目标已达成，不做"

### 1.1 验收目标出处与当前基线（审计 M1 更正）

| 指标 | 验收目标（出处） | 当前实测（P3-08 bench，Europe v4） | 裕度 |
|---|---|---|---|
| 路线典型 | **<1s**（§62 原文；§141 细化"单路线典型 <500ms"） | p99 0.378ms | **~2600×**（§62 口径） |
| 路线极端 | **<2s**（§62 原文；§142 多路线极端 <2s——不存在"最坏单次 <1s"条目） | max 0.603ms | **~3300×**（§62 口径） |
| 匹配 | 10-20Hz 更新（§62）；建议 p99<10ms（§140） | p99 0.012ms | ~830×（§140 口径） |
| 加载 | — | 327ms（一次性） | — |
| 常驻内存 | <500MB（§62） | 328MB（进程工作集实测，释放 junctions 后） | 1.5× |
| 限速查询 | p99<10µs（P3-09 细化，非 §62） | 0.2µs | 50× |

> 口径注：max 0.603ms 出自 P2-19 bench（Berlin 19 OD 样本——p99≡max 因 n=19 索引取整；
> P3-08 复跑 p99 0.378 为同批数据）；两数字不构成同一次运行，量级均为亚毫秒。
> P3-08 报告表引用"<500ms/<1s/<10ms"为 §141/142/§140 细化出处，本节按 §62 原文为准。

### 1.2 判定

**§62 是验收标准而非无条件工作项**（计划原文）。当前 A* 基线在 §62 真实口径下超验收
目标 3 个数量级（~2600×/~3300×），ALT/CH 分层路由的工程成本（图预处理、双向搜索适配、
着陆集维护）与收益（对已达标指标再快 N 倍）不匹配。**登记为"目标已达成，不做"**
（PLAN §2 A4 明确允许此形态）。真实裕度大于报告初版的 1300×/1600×（初版误引细化目标）——
判定方向不受影响，反而更稳。

### 1.3 触发重评条件（登记）

- 全图 OD 批量场景（如 P5 od-check 2000 对 × 3 profile）成为日常操作时：若需求增长到
  十万对级可考虑批处理级优化（非 ALT/CH——种子/共享计算）
- 移动端（B5）若实测路由延迟不可接受（当前无此迹象）

## 二、增量编译指纹（§9）——实现

### 2.1 现状

- manifest.json 已含 `game_version`（P2 §29 校验）
- GameInstall.Detect 已枚举 `dlc_*.scs`（含 dlc/ 子目录）并计算 ContentFingerprint
  （P1 §11：版本 + archive 名/大小/UTC 时间 + DLC 集合 → SHA-256）
- **缺失**：DLC 集合/archive 变更无检测——DLC 更新后旧 dataset 静默使用

### 2.2 实现（map-inspector `--check-fingerprint`）

- 指纹 = GameInstall.ContentFingerprint（SHA-256，P1 §11 复用）
- 写入 dataset manifest（additive 字段 **content_fingerprint**——向后兼容，零路由 schema 变更）
- 校验：当前安装指纹 vs manifest → 一致输出 `FINGERPRINT MATCH`；不一致输出
  `FINGERPRINT CHANGED stored=... cur=...`（完整值）+ exit 1（machine-readable ASCII）
- **验证记录（审计 M2 更正——与交付状态对齐）**：
  - 交付仓库状态（manifest 无 content_fingerprint 字段，未重建）→ 实测 `FINGERPRINT
    CHANGED stored=none cur=fa3ef5bb...`（**预期行为**：dataset 重建后字段落盘）
  - MATCH 路径仅在 manifest 已含正确指纹时成立（= 重建后的 dataset）——伪造正确指纹
    manifest 实测 MATCH、伪造错误指纹实测 CHANGED（差异值完整输出）——三路均与代码语义一致

### 2.3 边界与 §9 覆盖缺口（审计 M5 登记）

- archive 内容哈希不做（size+mtime 对游戏更新检测足够——Steam 更新会改 mtime）
- **§9 规范覆盖 4/9 项**：game_version ✓ / archive 集 ✓ / sector 集合（manifest.sectors）✓ /
  dataset 格式版本 ✓；**未覆盖**：mods/mod order（mod 安装不改安装目录 .scs 元数据——
  **指纹 MATCH 但地图数据已变——真实漏报路径**，登记 P2 遗留）、sector/definition 内容
  哈希、parser schema version——DLC/mods 场景建议 B3 后按需扩展
- 增量重建（只重建变更 sector）不做：全量重建 ~10 分钟（europe-scale），增量收益低且
  复杂度高（sector 依赖图）——登记为已知限制
- 检测后处置流程：本工具只检出变更，**重建命令 = map-inspector --all-sectors --dataset**
  （P1 既有流程）——不挂接任何 bat（初版"挂接 run-p5-tests.bat"声明删除——无实据）

## 三、验证记录

| 项 | 结果 |
|---|---|
| bench（Europe v4，P3-08 复述） | 路线 p99 0.378ms / max 0.603ms（P2-19 Berlin 19 OD 口径）/ 匹配 0.012ms / 限速 0.2µs |
| 指纹工具三路 | CHANGED（交付状态 stored=none）、MATCH（伪造正确指纹）、CHANGED（伪造错误指纹完整差异） |
| cargo 门 | fmt PASS / clippy 0 / 测试全绿 |

## 四、已知限制（登记）

- ALT/CH 不做（§1.2）；增量重建不做（§2.3）；指纹用 size+mtime（非哈希）
- §9 未覆盖：mods/mod order 漏报路径、sector/definition 哈希、parser schema version（§2.3）
- §63 正式性能验收以 B6 实机数据为最终依据（D6 延后）
