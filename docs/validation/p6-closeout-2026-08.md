# P6 关门报告（2026-08-12）

**阶段**：P6 性能优化与发布（v0.2 §62–63；PLAN-P3plus.md §2 A4）
**关门口径**：沿用 P2/P3 先例——机器可验证部分完成即关门（tag），实机验收（§63，B6）登记已知限制（D6 决策）。
**交付主线**：`0edcb2b`（性能评估 + 增量指纹）+ 审计修复 `7236f33` / `ba2c4d2` / `25092e6`。

---

## §1 阶段目标与出口条件

PLAN.md §4：P6 = 「性能优化（ALT/CH、增量编译等）与正式发布（v0.2 §62–63）」，入口条件 P5，出口验收「benchmark 目标达成」。

PLAN-P3plus.md §2 A4 拆分三项：ALT/CH 评估、增量编译（§9 地图更新检测）、§63 正式验收方法固化。

## §2 出口条件对照

| # | 出口条件（v0.2 §62–63） | 状态 | 证据 |
|---|---|---|---|
| 1 | §62 路线典型 <1 s | ✅ 超额 | A\* p99 **0.378 ms**（P3-08 复跑）/ 0.39 ms（P2-19）——裕度约 **2,600×** |
| 2 | §62 路线极端 <2 s | ✅ 超额 | max **0.39 ms**（P2-19，Berlin 19 OD，n=19 取整致 p99≡max）——裕度约 **5,100×** |
| 3 | §62 匹配 10–20 Hz（§140 建议 p99<10 ms） | ✅ 超额 | 匹配 p99 **0.012 ms**——裕度约 **830×** |
| 4 | §62 常驻内存 <500 MB | ✅ | **328 MB**（进程工作集实测，释放 junctions 后）——裕度 1.5× |
| 5 | 限速查询热路径（P3-09 细化 p99<10 µs） | ✅ 超额 | **0.2 µs**——裕度 **50×** |
| 6 | 加载时间 | ✅ | 327 ms（一次性） |
| 7 | ALT/CH 分层路由 | ✅（登记「目标已达成，不做」） | §3 判定 |
| 8 | 增量编译（§9 地图更新检测） | ✅（部分） | `map-inspector --check-fingerprint`；§9 覆盖 4/9 项，缺口登记 |
| 9 | §63 正式性能验收（实机） | ⚠️ 未做（登记） | 以 B6 实机数据为最终依据（D6 延后） |

## §3 ALT/CH 判定：登记「目标已达成，不做」

§62 是**验收标准**而非无条件工作项（计划原文）。当前 A\* 基线在 §62 真实口径下超验收目标 3 个数量级（约 2,600× / 5,100×），ALT/CH 分层路由的工程成本（图预处理、双向搜索适配、着陆集维护）与收益（对已达标指标再快 N 倍）不匹配。

审计更正记录（a4-perf-truth / a4-conclusion）：报告初版误引 §141/§142 细化目标（<500 ms / 多路线 <2 s）致裕度低估为 1,300×/1,600×；按 §62 原文重算为 2,600×/5,100×。判定方向不受影响。

**触发重评条件（登记）**：全图 OD 批量场景（如 od-check 2000 对 × 3 profile）成为日常操作、或需求增长到十万对级（此时考虑批处理级共享计算而非 ALT/CH）；移动端（B5）实测路由延迟不可接受（当前无此迹象）。

## §4 增量编译指纹（§9）

- 指纹 = `GameInstall.ContentFingerprint`（SHA-256：游戏版本 + archive 名/大小/UTC mtime + DLC 集合），P1-02 复用。
- 写入 dataset manifest（additive 字段 `content_fingerprint`，零路由 schema 变更）。
- 校验：`map-inspector --check-fingerprint --install <game> --dataset <dir>` → `FINGERPRINT MATCH`（exit 0）或 `FINGERPRINT CHANGED stored=… cur=…`（exit 1，machine-readable ASCII）。
- 三路实测与代码语义一致：交付态（manifest 无字段）→ CHANGED `stored=none`；伪造正确指纹 → MATCH；伪造错误指纹 → CHANGED（完整差异值）。

**检测后处置**：本工具只检出变更；重建命令 = `map-inspector --all-sectors --dataset <dir>`（P1 既有流程）。未挂接任何自动化脚本（初版报告曾声称「挂接 run-p5-tests.bat」，无实据，已删除）。

## §5 已知限制（登记）

1. **§9 覆盖 4/9 项**：已覆盖 game_version / archive 集 / sector 集（manifest.sectors）/ dataset 格式版本；**未覆盖** mods 与 mod order（mod 安装不改安装目录 `.scs` 元数据——**指纹 MATCH 但地图数据已变，为真实漏报路径**）、sector/definition 内容哈希、parser schema version。
2. **增量重建不做**：只重建变更 sector 需 sector 依赖图，复杂度高；全量重建约 10 分钟（europe-scale），增量收益低。
3. **指纹由 archive 名/大小/UTC mtime 推导**（SHA-256 摘要），**不哈希文件内容**。
4. **§63 正式验收未做**：PresentMon 管道（P0-D 方法）已就绪，实机对比（avg FPS 差异 ≤1–2%、1% low 回退 ≤2%）——B6。
5. **限速查询 0.2 µs 出处**：为 P3-09 报告数（非独立复测），量级一致。
6. **上游口径历史矛盾**：P2-closeout 曾将 0.39 ms 标为「Europe v4 全量」，实为 Berlin 核心网 19 OD 样本——p6 报告已注明，上游报告保留历史口径。

## §6 验证命令

```
cd E:\Projects\Pi\ETS2Nav
cd nav-core && cargo test -p nav-router bench        # 或 nav-core-cli bench <dataset>
.\tools\map-inspector\MapInspector\bin\Release\net9.0\map-inspector.exe ^
    --check-fingerprint --install "E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2" ^
    --dataset data\europe-v5
```

## §7 结论

P6 的 §62 性能验收标准在真实口径下全部超额达成（路线/匹配/内存三项均远超目标，最小裕度 1.5× 为内存），ALT/CH 经评估登记为「目标已达成，不做」；§9 增量编译实现指纹检测（覆盖 4/9 项，mods 漏报路径登记）；§63 正式验收以 B6 实机数据为最终依据。

**P6 关门（机器可验证部分完成 + 实机验收登记）。**
