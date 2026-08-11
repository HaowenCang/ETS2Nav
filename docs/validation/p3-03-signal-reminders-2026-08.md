# P3-03 红灯减速（§36）+ 即将绿灯（§37）验证报告（2026-08-11）

**工作包**：P3-03（P3-driving-assistant-plan.md §1）
**提交**：feat/p3-03-signal-reminders → main
**状态**：✅ 完成

---

## 一、实现（reminder.rs 扩展）

**§36 红灯减速提示**：

- `RedLightConfig`——停车距离模型参数（保守驾驶辅助，不追求卡车动力学精度）：
  默认 t_r=1.0s、a=2.5 m/s²、d_m=5m，可配置；
- `stopping_distance(v)` = v·t_r + v²/(2a) + d_m（§36 公式）；
- `red_light_warning(dist, v, state, cfg)`——**d_signal < d_stop(v) AND signal != GREEN**：
  - Red/Yellow 触发（Yellow 即将变红）；
  - **Green/Unknown 不触发**（Unknown 为读取失败，不产生误导性播报——比字面
    "signal != GREEN" 更保守）。

**§37 即将绿灯**：

- `GreenImminentConfig`——默认 max_speed 5 m/s（18 km/h）、max_remaining 3s，可配置；
- `green_imminent(state, confidence, v, remaining, cfg)`——四条件组合：
  state==RED && **confidence==Verified**（§115：只有 VERIFIED 可交给 P3 做
  countdown——即 §37 的 countdown_verified）&& v < 阈值（防诱导加速）&&
  remaining < 阈值 && remaining ≥ 0（负值时钟异常不触发）。

输入接口直接复用 P2-16 `signal::UpcomingSignal`（state/remaining_time/confidence）
——决策层零 IO，合成信号序列即可断言。

## 二、单元测试（10 个新增）

| 测试 | 断言 |
|---|---|
| stopping_distance_math | v=10 → 35m；v=20 → 105m（平方增长） |
| red_light_triggers_when_close | 30m<35m 触发；40m 不触发 |
| red_light_green_and_unknown_do_not_trigger | Green/Unknown 不触发 |
| red_light_yellow_triggers | Yellow 触发 |
| red_light_high_speed_earlier_warning | v=20 在 100m 触发（v=10 不触发） |
| green_imminent_all_conditions | Red+Verified+低速+短剩余 → 触发 |
| green_imminent_high_speed_suppressed | 高速接近不播报（防诱导加速） |
| green_imminent_long_remaining_suppressed | 剩余 8s 不触发 |
| green_imminent_unverified_suppressed | Probable/Unknown 不触发 |
| green_imminent_non_red_suppressed | Green/负剩余不触发 |

## 三、门与回归

- cargo fmt PASS；clippy 0 warnings；cargo test **76 全绿**（+10，无 FAILED）
- P1/P2 无受影响

## 四、已知限制

- 参数（t_r/a/d_m、速度/剩余阈值）为保守默认——B2 T3 信号 runtime 实测后校准
  （§36 明确"参数不追求仿真级精度"；§37 阈值体验在 B4 验收）；
- 触发-播报-去抖由 P3-06 事件流与频率管理处理（本包为单帧决策）。
