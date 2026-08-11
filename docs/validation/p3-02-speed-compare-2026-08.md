# P3-02 限速对照 diagnostic（§39）验证报告（2026-08-11）

**工作包**：P3-02（P3-driving-assistant-plan.md §1）
**提交**：feat/p3-02-speed-compare → main
**状态**：✅ 完成

---

## 一、实现

新增 `nav-core/crates/nav-router/src/reminder.rs`（提醒决策模块——P3-02~05 共用）：

- `SpeedLimitCompare`——限速对照结果（map/telemetry/diff/mismatch）；
- `compare_speed_limit(map_limit_kmh, telemetry_limit_ms)`——§39 运行时对照：
  - telemetry speed_limit 通道为 m/s（0 = 游戏无限速）→ 转 km/h 对照；
  - **map -1（未知/非 Road 边）不对照**（movement 边限速语义未定义，P3-01 实证）；
  - 容差 ±5 km/h（`SPEED_COMPARE_TOLERANCE_KMH`）——§102 明确容差由实测确定，取低于道路限速粒度避免传感器噪声误报；
  - 0（无限速）↔ 0 匹配；0 ↔ 80 判不一致。
- `OverSpeedConfig` + `overspeed_check`——§41 超速提醒：
  - 默认：≤50 区 +3 / >50 区 +5（§41 建议默认），可配置（低限/低偏移/高偏移）；
  - limit ≤ 0（未知/无限速）不提醒（无限速无超速语义、未知不猜测）；
  - 返回触发速度（km/h）供上层判定播报边界。

## 二、单元测试（10 个新增）

| 测试 | 断言 |
|---|---|
| compare_match | 13.9 m/s(50) vs map 50 → 一致 |
| compare_mismatch | 16.7 m/s(60) vs 50 → mismatch +10 |
| compare_within_tolerance | 52 vs 50 → 容差内一致 |
| compare_unlimited_both | 0 m/s vs map 0 → 一致（autobahn） |
| compare_unlimited_vs_limited | 0 m/s vs 80 → mismatch -80 |
| compare_unknown_map_skips | map -1 → 不对照 |
| overspeed_default_low | 50 区：53 不提醒 / 54 提醒 |
| overspeed_default_high | 80 区：85 不提醒 / 86 提醒 |
| overspeed_custom_config | 自定义阈值生效（60 区 +1、90 区 +10） |
| overspeed_unknown_unlimited | -1/0 不提醒 |

## 三、门与回归

- cargo fmt PASS；clippy 0 warnings；cargo test **66 全绿**（+10，无 FAILED）
- P1/P2 无受影响

## 四、说明与已知限制

- 本包为纯决策函数（无 IO）——B2 T1 实测时由 speed-validator 驱动采集对照结果，产出限速一致率（§39 "speed-limit parser 最有价值的 ground truth"）；
- 容差默认 ±5 待实测校准（B2 T1 数据到达后冻结）；
- movement 边对照跳过（map 语义未定义）——实测阶段确认游戏 HUD 在路口内的限速显示行为后可细化。
