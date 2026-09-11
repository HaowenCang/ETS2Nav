// P3-02~05：提醒决策模块（v0.2 §39/§41/§36/§37/§38）。
// 本文件：§39 限速对照 + §41 超速提醒 + §36 红灯减速 + §37 即将绿灯 + §38 GLOSA。
// 纯决策逻辑（输入快照 → 输出提醒事件），不依赖 UI/TTS（P3-06 语义分离）。

use crate::signal::{LightState, SignalConfidence};

/// 限速对照结果（§39）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedLimitCompare {
    /// 地图限速（km/h；-1 = 未知/无 map 语义）。
    pub map_limit_kmh: i16,
    /// 游戏遥测限速（km/h；0 = 游戏无限速；-1 = 遥测未提供）。
    pub telemetry_limit_kmh: i16,
    /// 是否不一致（超出容差）。
    pub mismatch: bool,
    /// telemetry - map（km/h）。
    pub diff_kmh: i16,
}

/// 对照容差（km/h）。§39 未定值——P1-09 §102 明确"具体容差应由实测数据确定"，
/// 此处取 ±5 为默认（低于实际道路限速粒度，避免传感器噪声误报）。
pub const SPEED_COMPARE_TOLERANCE_KMH: i16 = 5;

/// §39 当前限速对照：telemetry speed_limit（m/s，0 = 无限速）vs map edge 限速。
///
/// - map -1（未知/非 Road 边）：不对照（map_limit_kmh = -1，mismatch = false）；
/// - telemetry 0（游戏无限速）视为 0 km/h 参与对照；
/// - |diff| <= 容差视为一致。
pub fn compare_speed_limit(map_limit_kmh: i16, telemetry_limit_ms: f32) -> SpeedLimitCompare {
    if map_limit_kmh < 0 {
        return SpeedLimitCompare {
            map_limit_kmh,
            telemetry_limit_kmh: -1,
            mismatch: false,
            diff_kmh: 0,
        };
    }
    if !telemetry_limit_ms.is_finite() {
        // NaN/Inf 源（审计 MINOR-5）：不对照，避免虚假告警
        return SpeedLimitCompare {
            map_limit_kmh,
            telemetry_limit_kmh: -1,
            mismatch: false,
            diff_kmh: 0,
        };
    }
    let tel_kmh = (telemetry_limit_ms * 3.6).round() as i16;
    let diff = tel_kmh - map_limit_kmh;
    SpeedLimitCompare {
        map_limit_kmh,
        telemetry_limit_kmh: tel_kmh,
        mismatch: diff.abs() > SPEED_COMPARE_TOLERANCE_KMH,
        diff_kmh: diff,
    }
}

/// 超速提醒决策（§41）。默认阈值：≤50 km/h 时 +3、>50 时 +5；可配置。
/// 返回建议提醒时的触发速度（km/h）；speed 未超阈值返回 None。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverSpeedConfig {
    /// 低限速段阈值（≤low_limit km/h 时用 low_offset）。
    pub low_limit_kmh: i16,
    pub low_offset_kmh: i16,
    pub high_offset_kmh: i16,
}

impl Default for OverSpeedConfig {
    fn default() -> Self {
        OverSpeedConfig {
            low_limit_kmh: 50,
            low_offset_kmh: 3,
            high_offset_kmh: 5,
        }
    }
}

/// 判定当前速度是否触发超速提醒。
/// limit <= 0（未知/无限速）：无限速不提醒；未知（-1）不提醒。
pub fn overspeed_check(speed_ms: f32, limit_kmh: i16, cfg: &OverSpeedConfig) -> Option<i16> {
    if limit_kmh <= 0 {
        return None;
    }
    let speed_kmh = (speed_ms * 3.6).round() as i16;
    let offset = if limit_kmh <= cfg.low_limit_kmh {
        cfg.low_offset_kmh
    } else {
        cfg.high_offset_kmh
    };
    let trigger = limit_kmh + offset;
    (speed_kmh > trigger).then_some(trigger)
}

// ─── §36 红灯减速提示 ─────────────────────────────────────────────────────

/// §36 停车距离模型参数（保守驾驶辅助——不追求卡车动力学仿真精度）。
/// d_stop = v·t_r + v²/(2a) + d_m。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RedLightConfig {
    /// 反应时间 t_r（s）。
    pub reaction_time_s: f32,
    /// 减速度 a（m/s²）。
    pub deceleration: f32,
    /// 安全余量 d_m（m）。
    pub safety_margin_m: f32,
}

impl Default for RedLightConfig {
    fn default() -> Self {
        RedLightConfig {
            reaction_time_s: 1.0,
            deceleration: 2.5,
            safety_margin_m: 5.0,
        }
    }
}

/// §36 停车距离 d_stop = v·t_r + v²/(2a) + d_m（v 单位 m/s，输出米）。
pub fn stopping_distance(v_ms: f32, cfg: &RedLightConfig) -> f32 {
    v_ms * cfg.reaction_time_s + v_ms * v_ms / (2.0 * cfg.deceleration) + cfg.safety_margin_m
}

/// §36 红灯减速触发：d_signal < d_warning(v) AND signal != GREEN。
/// Unknown 状态不触发（读取失败不产生误导性播报）；Yellow 触发（即将变红）。
pub fn red_light_warning(
    distance_to_stop_line_m: f32,
    v_ms: f32,
    state: LightState,
    cfg: &RedLightConfig,
) -> bool {
    if state != LightState::Red && state != LightState::Yellow {
        return false;
    }
    distance_to_stop_line_m >= 0.0 && distance_to_stop_line_m < stopping_distance(v_ms, cfg)
}

// ─── §37 即将绿灯 ─────────────────────────────────────────────────────────

/// §37 即将绿灯条件参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GreenImminentConfig {
    /// 车辆速度阈值（m/s）——高于此值不播报（防诱导加速）。
    pub max_speed_ms: f32,
    /// 剩余红灯时间阈值（s）——低于此值才播报。
    pub max_remaining_s: f32,
}

impl Default for GreenImminentConfig {
    fn default() -> Self {
        GreenImminentConfig {
            max_speed_ms: 5.0, // 18 km/h
            max_remaining_s: 3.0,
        }
    }
}

/// §37 即将绿灯：state==RED && countdown_verified && speed < threshold &&
/// remaining < threshold。countdown_verified = SignalConfidence::Verified
/// （§115：只有 VERIFIED 可交给 P3 做 countdown/GLOSA）。
pub fn green_imminent(
    state: LightState,
    confidence: SignalConfidence,
    v_ms: f32,
    remaining_s: f64,
    cfg: &GreenImminentConfig,
) -> bool {
    state == LightState::Red
        && confidence == SignalConfidence::Verified
        && v_ms < cfg.max_speed_ms
        && remaining_s < cfg.max_remaining_s as f64
        && remaining_s >= 0.0
}

// ─── §38 GLOSA ─────────────────────────────────────────────────────────────

/// §38 GLOSA 参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlosaConfig {
    /// 绿灯时长 G（s）。数据源实证（P3-00）：dataset v2 仅存 signal_group_type
    /// profile 名，未存 interval——绿灯时长以配置默认提供，B2 T3 实测后校准。
    pub green_duration_s: f32,
    /// 合理加速度（m/s²）。
    pub max_accel: f32,
    /// 距停止线过近（m）不输出（刹车距离内无意义）。
    pub min_distance_m: f32,
    /// 距停止线过远（m）不输出（窗口估计不确定度随距离增大）。
    pub max_distance_m: f32,
}

impl Default for GlosaConfig {
    fn default() -> Self {
        GlosaConfig {
            green_duration_s: 15.0,
            max_accel: 2.0,
            min_distance_m: 20.0,
            max_distance_m: 500.0,
        }
    }
}

/// GLOSA 建议（§38 低精度速度区间）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlosaAdvice {
    /// 区间下限（km/h，5 的倍数向下取整）。
    pub v_min_kmh: i16,
    /// 区间上限（km/h，5 的倍数向上取整）。
    pub v_max_kmh: i16,
    /// 窗口与物理约束交集非空。
    pub feasible: bool,
}

/// §38 GLOSA：绿灯通行窗口 [t1, t2] × 距离 d → 理论速度范围 d/t2 ≤ v ≤ d/t1，
/// 与限速（当前与前方取低）及合理加速度求交集，输出低精度区间。
///
/// - 红灯：窗口 = [remaining, remaining + G]（remaining 为到绿灯开始）；
/// - 绿灯：窗口 = [0, remaining]（remaining 为绿灯剩余）；
/// - 黄/未知：不输出（feasible=false）；
/// - 加速可达上限 v ≤ sqrt(v_now² + 2·a·d)（恒定加速度近似）；
/// - 量化：下限 floor 到 5 km/h、上限 ceil 到 5 km/h（§38 不制造虚假精度）。
pub fn glosa_advice(
    distance_m: f32,
    state: LightState,
    confidence: SignalConfidence,
    remaining_s: f64,
    v_now_ms: f32,
    limit_kmh: i16,
    cfg: &GlosaConfig,
) -> GlosaAdvice {
    if confidence != SignalConfidence::Verified
        || !distance_m.is_finite()
        || !remaining_s.is_finite()
        || !v_now_ms.is_finite()
        || distance_m < cfg.min_distance_m
        || distance_m > cfg.max_distance_m
        || remaining_s < 0.0
    {
        return GlosaAdvice {
            v_min_kmh: 0,
            v_max_kmh: 0,
            feasible: false,
        };
    }
    // 窗口 [t1, t2]（秒）
    let (t1, t2) = match state {
        LightState::Red => (
            remaining_s as f32,
            remaining_s as f32 + cfg.green_duration_s,
        ),
        LightState::Green => (0.0, remaining_s as f32),
        _ => {
            return GlosaAdvice {
                v_min_kmh: 0,
                v_max_kmh: 0,
                feasible: false,
            }
        }
    };
    if t2 <= 0.0 || t2 < t1 {
        return GlosaAdvice {
            v_min_kmh: 0,
            v_max_kmh: 0,
            feasible: false,
        };
    }
    // 理论速度范围（m/s）
    let v_lo = distance_m / t2; // d/t2 ≤ v
    let mut v_hi = if t1 > 0.0 {
        distance_m / t1
    } else {
        f32::INFINITY
    };
    // 物理约束：加速可达上限 + 限速 cap（当前与前方取低）
    let v_acc_hi = (v_now_ms * v_now_ms + 2.0 * cfg.max_accel * distance_m).sqrt();
    v_hi = v_hi.min(v_acc_hi);
    if limit_kmh > 0 {
        v_hi = v_hi.min(limit_kmh as f32 / 3.6);
    }
    if v_lo > v_hi {
        return GlosaAdvice {
            v_min_kmh: 0,
            v_max_kmh: 0,
            feasible: false,
        };
    }
    // 低精度量化（§38：53.6~64.2 → 50~65 式显示）；epsilon 对称吸收 f32 边界误差
    // （审计 M2 修复：floor 方向裸除会使恰为 5 倍数的 v_min 低估一档——v_min 是
    // "必须不低于"的硬约束，低估不安全；两方向各取 ±epsilon）
    let q = |x: f32, up: bool| -> i16 {
        let kmh = x * 3.6;
        if up {
            (((kmh - 1e-3) / 5.0).ceil() as i16 * 5).max(5)
        } else {
            (((kmh + 1e-3) / 5.0).floor() as i16 * 5).max(0)
        }
    };
    GlosaAdvice {
        v_min_kmh: q(v_lo, false),
        v_max_kmh: q(v_hi, true),
        feasible: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_match() {
        // 13.9 m/s = 50 km/h == map 50
        let r = compare_speed_limit(50, 13.9);
        assert!(!r.mismatch);
        assert_eq!(r.map_limit_kmh, 50);
        assert_eq!(r.telemetry_limit_kmh, 50);
    }

    #[test]
    fn compare_mismatch() {
        // 16.7 m/s = 60 km/h vs map 50 → +10
        let r = compare_speed_limit(50, 16.7);
        assert!(r.mismatch);
        assert_eq!(r.diff_kmh, 10);
    }

    #[test]
    fn compare_within_tolerance() {
        // 14.4 m/s = 52 km/h vs 50 → +2 ≤ 5 容差内
        let r = compare_speed_limit(50, 14.4);
        assert!(!r.mismatch);
    }

    #[test]
    fn compare_unlimited_both() {
        // 游戏无限速 0 m/s == map 0（autobahn）
        let r = compare_speed_limit(0, 0.0);
        assert!(!r.mismatch);
    }

    #[test]
    fn compare_unlimited_vs_limited() {
        // 游戏无限速 vs map 80 → 不一致
        let r = compare_speed_limit(80, 0.0);
        assert!(r.mismatch);
        assert_eq!(r.diff_kmh, -80);
    }

    #[test]
    fn compare_unknown_map_skips() {
        // map -1（未知/movement 边）：不对照
        let r = compare_speed_limit(-1, 13.9);
        assert!(!r.mismatch);
        assert_eq!(r.telemetry_limit_kmh, -1);
    }

    #[test]
    fn overspeed_default_low() {
        // 50 区：53 不提醒、54 提醒（+3）
        let cfg = OverSpeedConfig::default();
        assert_eq!(overspeed_check(14.72, 50, &cfg), None); // 53.0
        assert_eq!(overspeed_check(15.0, 50, &cfg), Some(53)); // 54.0
    }

    #[test]
    fn overspeed_default_high() {
        // 80 区：85 不提醒、86 提醒（+5）
        let cfg = OverSpeedConfig::default();
        assert_eq!(overspeed_check(23.61, 80, &cfg), None); // 85.0
        assert_eq!(overspeed_check(24.0, 80, &cfg), Some(85)); // 86.4
    }

    #[test]
    fn overspeed_custom_config() {
        let cfg = OverSpeedConfig {
            low_limit_kmh: 60,
            low_offset_kmh: 1,
            high_offset_kmh: 10,
        };
        assert_eq!(overspeed_check(18.1, 60, &cfg), Some(61)); // 65 > 61
        assert_eq!(overspeed_check(28.0, 90, &cfg), Some(100)); // 100.8 > 100
    }

    #[test]
    fn overspeed_unknown_unlimited() {
        let cfg = OverSpeedConfig::default();
        assert_eq!(overspeed_check(30.0, -1, &cfg), None); // 未知
        assert_eq!(overspeed_check(30.0, 0, &cfg), None); // 无限速
    }

    // ─── §36 红灯减速 ───

    #[test]
    fn stopping_distance_math() {
        // v=10 m/s, t_r=1s, a=2.5 m/s², d_m=5m → 10 + 100/5 + 5 = 35m
        let cfg = RedLightConfig::default();
        assert!((stopping_distance(10.0, &cfg) - 35.0).abs() < 1e-3);
        // v=20 → 20 + 400/5 + 5 = 105m（速度平方增长）
        assert!((stopping_distance(20.0, &cfg) - 105.0).abs() < 1e-3);
    }

    #[test]
    fn red_light_triggers_when_close() {
        let cfg = RedLightConfig::default();
        // v=10 → d_stop=35m；30m 处红灯触发
        assert!(red_light_warning(30.0, 10.0, LightState::Red, &cfg));
        // 40m 处不触发
        assert!(!red_light_warning(40.0, 10.0, LightState::Red, &cfg));
    }

    #[test]
    fn red_light_green_and_unknown_do_not_trigger() {
        let cfg = RedLightConfig::default();
        assert!(!red_light_warning(10.0, 10.0, LightState::Green, &cfg));
        assert!(!red_light_warning(10.0, 10.0, LightState::Unknown, &cfg));
    }

    #[test]
    fn red_light_yellow_triggers() {
        let cfg = RedLightConfig::default();
        assert!(red_light_warning(20.0, 10.0, LightState::Yellow, &cfg));
    }

    #[test]
    fn red_light_high_speed_earlier_warning() {
        let cfg = RedLightConfig::default();
        // v=20 → d_stop=105m；100m 处触发，且比 v=10 时远得多
        assert!(red_light_warning(100.0, 20.0, LightState::Red, &cfg));
        assert!(!red_light_warning(100.0, 10.0, LightState::Red, &cfg));
    }

    // ─── §37 即将绿灯 ───

    #[test]
    fn green_imminent_all_conditions() {
        let cfg = GreenImminentConfig::default();
        assert!(green_imminent(
            LightState::Red,
            SignalConfidence::Verified,
            2.0, // 低速
            1.5, // 剩余短
            &cfg
        ));
    }

    #[test]
    fn green_imminent_high_speed_suppressed() {
        // 高速接近不播报（防诱导加速）
        let cfg = GreenImminentConfig::default();
        assert!(!green_imminent(
            LightState::Red,
            SignalConfidence::Verified,
            15.0,
            1.5,
            &cfg
        ));
    }

    #[test]
    fn green_imminent_long_remaining_suppressed() {
        let cfg = GreenImminentConfig::default();
        assert!(!green_imminent(
            LightState::Red,
            SignalConfidence::Verified,
            2.0,
            8.0,
            &cfg
        ));
    }

    #[test]
    fn green_imminent_unverified_suppressed() {
        // 未验证倒计时（Probable/Unknown）不播报
        let cfg = GreenImminentConfig::default();
        assert!(!green_imminent(
            LightState::Red,
            SignalConfidence::Probable,
            2.0,
            1.5,
            &cfg
        ));
        assert!(!green_imminent(
            LightState::Red,
            SignalConfidence::Unknown,
            2.0,
            1.5,
            &cfg
        ));
    }

    #[test]
    fn green_imminent_non_red_suppressed() {
        let cfg = GreenImminentConfig::default();
        assert!(!green_imminent(
            LightState::Green,
            SignalConfidence::Verified,
            2.0,
            1.5,
            &cfg
        ));
        // 负剩余（时钟异常）也不触发
        assert!(!green_imminent(
            LightState::Red,
            SignalConfidence::Verified,
            2.0,
            -1.0,
            &cfg
        ));
    }

    // ─── §38 GLOSA ───

    #[test]
    fn glosa_window_math() {
        // 红灯剩余 5.6s、G=15 → 窗口 [5.6, 20.6]；d=100m：
        // v ∈ [100/20.6, 100/5.6] = [4.85, 17.86] m/s = [17.5, 64.3] km/h
        // 加速上限（v_now=10, a=2, d=100）：sqrt(100+400)=22.4 m/s 不压；
        // 限速 80 不压 → 量化 [15, 65]
        let cfg = GlosaConfig::default();
        let a = glosa_advice(
            100.0,
            LightState::Red,
            SignalConfidence::Verified,
            5.6,
            10.0,
            80,
            &cfg,
        );
        assert!(a.feasible);
        assert_eq!(a.v_min_kmh, 15);
        assert_eq!(a.v_max_kmh, 65);
    }

    #[test]
    fn glosa_limit_caps_upper() {
        // 同上但限速 50 → 上限压到 50（量化 50）；下限不变
        let cfg = GlosaConfig::default();
        let a = glosa_advice(
            100.0,
            LightState::Red,
            SignalConfidence::Verified,
            5.6,
            10.0,
            50,
            &cfg,
        );
        assert!(a.feasible);
        assert_eq!(a.v_max_kmh, 50);
    }

    #[test]
    fn glosa_green_phase() {
        // 绿灯剩余 8s、d=100 → v ∈ [100/8, ∞) = [45, ∞) km/h；限速 60 cap
        let cfg = GlosaConfig::default();
        let a = glosa_advice(
            100.0,
            LightState::Green,
            SignalConfidence::Verified,
            8.0,
            10.0,
            60,
            &cfg,
        );
        assert!(a.feasible);
        assert_eq!(a.v_min_kmh, 45);
        assert_eq!(a.v_max_kmh, 60);
    }

    #[test]
    fn glosa_unreachable_window() {
        // 绿灯仅剩 2s、d=200m → v_min = 100 m/s = 360 km/h > 限速 80 → 不可行
        let cfg = GlosaConfig::default();
        let a = glosa_advice(
            200.0,
            LightState::Green,
            SignalConfidence::Verified,
            2.0,
            10.0,
            80,
            &cfg,
        );
        assert!(!a.feasible);
    }

    #[test]
    fn glosa_accel_caps_upper() {
        // 绿灯剩余充足（60s）→ 窗口上限 ∞，加速可达性生效：
        // v_now=0、d=100、a=2 → sqrt(0+400)=20 m/s=72 km/h → 量化 75
        let cfg = GlosaConfig::default();
        let a = glosa_advice(
            100.0,
            LightState::Green,
            SignalConfidence::Verified,
            60.0,
            0.0,
            80,
            &cfg,
        );
        assert!(a.feasible);
        assert_eq!(a.v_max_kmh, 75); // 72 ceil 到 75
    }

    #[test]
    fn glosa_vmin_boundary_floor_epsilon() {
        // 审计 M2 回归：d=25/t2=3 绿灯 → v_lo=25/3=8.333 m/s=30.0 km/h
        // f32 管线可能落在 29.999998——floor 裸除会低估到 25（不安全方向）；
        // 对称 epsilon 修复后必须 30
        let cfg = GlosaConfig::default();
        let a = glosa_advice(
            25.0,
            LightState::Green,
            SignalConfidence::Verified,
            3.0,
            0.0, // v_now=0 → v_acc_hi=10 m/s ≥ v_lo=8.33，可行
            80,
            &cfg,
        );
        assert!(a.feasible);
        assert_eq!(a.v_min_kmh, 30);
    }

    #[test]
    fn glosa_unverified_or_bad_state() {
        let cfg = GlosaConfig::default();
        // 未验证
        assert!(
            !glosa_advice(
                100.0,
                LightState::Red,
                SignalConfidence::Probable,
                5.6,
                10.0,
                80,
                &cfg
            )
            .feasible
        );
        // 黄灯
        assert!(
            !glosa_advice(
                100.0,
                LightState::Yellow,
                SignalConfidence::Verified,
                5.6,
                10.0,
                80,
                &cfg
            )
            .feasible
        );
        // 距离过近（<20m）
        assert!(
            !glosa_advice(
                10.0,
                LightState::Green,
                SignalConfidence::Verified,
                8.0,
                10.0,
                60,
                &cfg
            )
            .feasible
        );
        // 距离过远（>500m）
        assert!(
            !glosa_advice(
                600.0,
                LightState::Green,
                SignalConfidence::Verified,
                8.0,
                10.0,
                60,
                &cfg
            )
            .feasible
        );
    }

    #[test]
    fn glosa_feasible_invariants_hold_across_sweep() {
        // P4R-2 数据契约前提：feasible 的建议必须满足 0 ≤ min ≤ max 且均为 5 的
        // 倍数。server 的 JSON 投影直接暴露这两个 i16，UI 不做二次修正，因此
        // 「UI 不得输出负数 / min>max」的前提必须由本函数保证，而不是由 UI 兜底。
        let cfg = GlosaConfig::default();
        let mut feasible_count = 0;
        for state in [LightState::Red, LightState::Green] {
            for d in [20.0f32, 21.0, 50.0, 100.0, 250.0, 499.0, 500.0] {
                for rem in [0.0f64, 0.5, 2.0, 5.6, 15.0, 40.0, 120.0] {
                    for v in [0.0f32, 5.0, 13.9, 30.0] {
                        for limit in [0i16, 30, 50, 80, 130] {
                            let a = glosa_advice(
                                d,
                                state,
                                SignalConfidence::Verified,
                                rem,
                                v,
                                limit,
                                &cfg,
                            );
                            if !a.feasible {
                                continue;
                            }
                            feasible_count += 1;
                            assert!(a.v_min_kmh >= 0, "负下限: {a:?} d={d} rem={rem}");
                            assert!(a.v_max_kmh >= 0, "负上限: {a:?} d={d} rem={rem}");
                            assert!(a.v_min_kmh <= a.v_max_kmh, "min>max: {a:?} d={d} rem={rem}");
                            assert_eq!(a.v_min_kmh % 5, 0, "下限未量化到 5: {a:?}");
                            assert_eq!(a.v_max_kmh % 5, 0, "上限未量化到 5: {a:?}");
                        }
                    }
                }
            }
        }
        assert!(
            feasible_count > 100,
            "扫描样本过少，覆盖无效: {feasible_count}"
        );
    }
}
