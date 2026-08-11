# P3-06 提醒事件流 + TTS（§48/§49）验证报告（2026-08-11）

**工作包**：P3-06（P3-driving-assistant-plan.md §1）
**提交**：feat/p3-06-reminder-speech → main
**状态**：✅ 完成

---

## 一、实现

新增 `nav-core/crates/nav-router/src/speak.rs`：

1. **ReminderEvent 语义枚举**（§48 核心层产物，UI/TTS 消费端各自转换）：
   `SpeedLimitChange{distance,limit}` / `OverSpeed{limit}` / `RedLight{distance}` /
   `GreenImminent` / `Glosa{v_min,v_max}`——与 P3-02~05 决策函数输出对接。

2. **§49 播报频率管理**：
   - `speak_ahead_distance_m(v, road_class, complexity, cfg)`——
     D = v×10 × class_factor × complex_factor，clamp [100, 2000]；
     class_factor：motorway 1.0 / express 0.8 / local 0.5；
     complexity ≥3（环岛/多出口）×1.3；档位缩放：低频 ×0.5 / 标准 ×1 / 高频 ×1.5；
   - `ReminderGate`——同类提醒最小间隔 30s（§48 防轰炸），不同类型独立计时。

3. **中文语音文本**（§48 "V1 中文必须完成"）：`to_speech_zh`——"前方3百米，限速50"
   / "前方5百米，解除限速" / "您已超速，当前限速50" / "前方250米，红灯，请减速" /
   "即将绿灯" / "建议保持50到60"；距离整百用"百米"简化（避免"米米"重复）。

4. **TTS 通道 smoke 验证**：Windows 离线 SAPI（PowerShell System.Speech 包装，
   零新增依赖）——`SAPI OK, voices=3`（3 个系统语音，含中文语音）。实际播报由
   UI 层消费（P4 A2 接入），本包验证通道可用性。

## 二、单元测试（5 个新增）

| 测试 | 断言 |
|---|---|
| speak_distance_standard | 城市 50 local → 250m；高速 90 motorway → 900m；快速路 80 express → 640m |
| speak_distance_clamp_and_complexity | 低速 clamp 下限 100m；130 km/h → 1300m；200 km/h → 2000m 上限；环岛 ×1.3 → 325m |
| speak_distance_frequency_scale | 低频 450m / 高频 1350m |
| reminder_gate_interval | 30s 内拒绝、之后放行；不同类型互不影响 |
| speech_zh_texts | 五类提醒中文文本逐字断言 |

（修复过程：clamp 顺序先 factor 后 clamp；ceil 量化浮点边界；测试断言同步）

## 三、门与回归

- cargo fmt PASS；clippy 0 warnings；cargo test **87 全绿**（+5，无 FAILED）
- P1/P2 无受影响

## 四、已知限制

- 提前距离参数（class/complexity 系数、clamp 界）为默认设计值——B4 实机听感验收后校准；
- 最小间隔 30s 为默认——B4 体验后调整；
- SAPI 中文语音依赖系统安装的语音包（本机 3 个 voice 可用）；
- TTS 实际发声由 UI 层触发（本包为语义 + 文本 + 通道验证）。
