// P3-02~05：提醒决策模块（v0.2 §39/§41/§36/§37/§38）。
// 本文件：§39 当前限速对照 diagnostic + §41 超速提醒。
// 纯决策逻辑（输入快照 → 输出提醒事件），不依赖 UI/TTS（P3-06 语义分离）。

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
}
