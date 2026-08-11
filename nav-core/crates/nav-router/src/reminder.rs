// P3-02~05：提醒决策模块（v0.2 §39/§41/§36/§37/§38）。
// 本文件：§39 限速对照 + §41 超速提醒 + §36 红灯减速 + §37 即将绿灯。
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
    distance_to_stop_line_m < stopping_distance(v_ms, cfg)
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
}
