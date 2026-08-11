# P6 性能评估报告（A4，2026-08-12）

**目标**：PLAN-P3plus.md §2 A4（P6 性能优化：ALT/CH 分层路由评估 + 增量编译指纹）
**依据**：v0.2 §62（验收标准）、§9（地图更新检测）、P1-02（fingerprint 基础）、P2-19/P3-08 bench 数据

## 一、ALT/CH 分层路由——评估结论：登记"目标已达成，不做"

### 1.1 §62 验收标准 vs 当前基线

| 指标 | §62 验收目标 | 当前实测（P3-08 bench，Europe v4 全图） | 裕度 |
|---|---|---|---|
| 路线计算（典型） | <500ms | **p99 0.378ms** | ~1300× |
| 路线计算（最坏单次） | <1s | max 0.603ms | ~1600× |
| 匹配（p99） | <10ms | 0.012ms | ~830× |
| 加载 | — | 327ms（一次性） | — |
| 常驻内存 | <500MB | 328MB（进程工作集实测，释放 junctions 后） | 1.5× |
| 限速查询（p99） | <10µs | 0.2µs | 50× |

### 1.2 判定

**§62 是验收标准而非无条件工作项**（计划原文）。当前 A* 基线已超验收目标 3 个数量级，
ALT/CH 分层路由的工程成本（图预处理、双向搜索适配、着陆集维护、~2 周工作量）与收益
（对已经 1300× 达标的指标再快 N 倍）不匹配。**登记为"目标已达成，不做"**（PLAN §2 A4
明确允许此形态）。

### 1.3 触发重评条件（登记）

- 全图 OD 批量场景（如 P5 od-check 2000 对 × 3 profile）成为日常操作时：当前 2000 对
  全图检查约数分钟，若需求增长到十万对级可考虑批处理级优化（非 ALT/CH，而是种子/共享
  计算）——见 §3 待议
- 移动端（B5）若实测路由延迟不可接受（当前无此迹象）

## 二、增量编译指纹（§9）——实现

### 2.1 现状

- manifest.json 已含 `game_version`（P2 §29 校验）
- GameInstall.Detect 已枚举 `dlc_*.scs`（含 dlc/ 子目录）
- **缺失**：DLC 集合/archive 变更无检测——DLC 更新后旧 dataset 静默使用（游戏版本一致但
  地图数据已变）

### 2.2 实现（map-inspector `--check-fingerprint`）

- 指纹 = { game_version, dlc: [(name, size, mtime), ...] }
- 写入 dataset manifest（additive 字段 `dlc_fingerprint`——向后兼容，零路由 schema 变更）
- 校验：当前安装指纹 vs manifest → 一致输出 `FINGERPRINT MATCH`；不一致列出差异项输出
  `FINGERPRINT CHANGED: ...`（machine-readable ASCII）
- 挂接：run-p5-tests.bat 可选步骤 + 文档说明重建命令

### 2.3 边界

- archive 内容哈希不做（size+mtime 对游戏更新检测足够——Steam 更新会改 mtime）
- 增量重建（只重建变更 sector）不做：当前全量重建 ~10 分钟（europe-scale），增量收益
  低且复杂度高（sector 依赖图）——登记为已知限制

## 三、验证记录

| 项 | 结果 |
|---|---|
| bench（Europe v4） | 路线 p99 0.378ms / max 0.603ms / 匹配 0.012ms / 限速 0.2µs（P3-08 实测复述） |
| 指纹工具 | 当前安装 vs europe-v4 manifest → MATCH；伪造变更 → CHANGED 检出（测试） |
| cargo 门 | fmt PASS / clippy 0 / 测试全绿 |

## 四、已知限制（登记）

- 增量重建不做（§2.3）；ALT/CH 不做（§1.2）；指纹用 size+mtime（非哈希）
- §63 正式性能验收以 B6 实机数据为最终依据（D6 延后）
