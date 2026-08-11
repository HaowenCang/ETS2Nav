// P3-06：提醒事件流 + 播报频率管理 + TTS 语义（v0.2 §48/§49）。
// §48：核心层只生成语义（type/distance/...），语音客户端转换（中文 V1）。
// §49：D=f(v, road_class, maneuver complexity) 计算播报提前距离；
// 同类提醒最小间隔防轰炸。
// TTS 通道：Windows 离线 SAPI（PowerShell 包装），本模块只生成语音文本。

/// 提醒事件语义（§48 核心层产物，UI/TTS 消费端各自转换）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderEvent {
    /// 前方限速变化（前方 distance_m 处限速 limit_kmh）。
    SpeedLimitChange { distance_m: u32, limit_kmh: i16 },
    /// 超速提醒（当前限速 limit_kmh）。
    OverSpeed { limit_kmh: i16 },
    /// 红灯减速。
    RedLight { distance_m: u32 },
    /// 即将绿灯。
    GreenImminent,
    /// GLOSA 建议区间（低精度）。
    Glosa { v_min_kmh: i16, v_max_kmh: i16 },
}

/// §49 播报频率档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrequencyProfile {
    Low,
    Standard,
    High,
}

/// §49 播报频率管理配置。
#[derive(Debug, Clone, Copy)]
pub struct SpeakConfig {
    pub frequency: FrequencyProfile,
    /// 同类提醒最小间隔（s）——防轰炸。
    pub min_interval_s: f32,
}

impl Default for SpeakConfig {
    fn default() -> Self {
        SpeakConfig {
            frequency: FrequencyProfile::Standard,
            min_interval_s: 30.0,
        }
    }
}

/// §49 提前距离 D=f(v, road_class, maneuver complexity)（米）。
/// 模型：D = clamp(v×k, min, max) × class_factor；档位缩放全局距离。
/// - motorway：×1.0（高速下需要更长提前量）
/// - express：×0.8
/// - local：×0.5（城市内短提前量）
///
/// 参数为默认设计值，B4 实机验收后校准。
pub fn speak_ahead_distance_m(
    v_kmh: f32,
    road_class: u8,
    complexity: u32,
    cfg: &SpeakConfig,
) -> u32 {
    let class_factor = match road_class {
        3 => 1.0, // motorway
        2 => 0.8, // express
        _ => 0.5, // local/unknown
    };
    // 复杂路口（环岛/多出口）加大提前量
    let complex_factor = if complexity >= 3 { 1.3 } else { 1.0 };
    let base = (v_kmh * 10.0) * class_factor * complex_factor;
    let scaled = match cfg.frequency {
        FrequencyProfile::Low => base * 0.5,
        FrequencyProfile::Standard => base,
        FrequencyProfile::High => base * 1.5,
    };
    scaled.clamp(100.0, 2000.0) as u32
}

/// 同类提醒最小间隔管理（§48 防轰炸）。
#[derive(Debug, Clone)]
pub struct ReminderGate {
    last: std::collections::HashMap<&'static str, f64>,
}

impl Default for ReminderGate {
    fn default() -> Self {
        Self::new()
    }
}

impl ReminderGate {
    pub fn new() -> Self {
        ReminderGate {
            last: std::collections::HashMap::new(),
        }
    }

    /// 距上次同类提醒不足最小间隔则拒绝（返回 false）。
    pub fn allow(&mut self, kind: &'static str, now_s: f64, cfg: &SpeakConfig) -> bool {
        let last = self.last.get(kind).copied().unwrap_or(f64::NEG_INFINITY);
        if now_s - last < cfg.min_interval_s as f64 {
            return false;
        }
        self.last.insert(kind, now_s);
        true
    }
}

/// §48 语义 → 中文语音文本（V1 中文必须完成）。
pub fn to_speech_zh(ev: &ReminderEvent) -> String {
    match ev {
        ReminderEvent::SpeedLimitChange {
            distance_m,
            limit_kmh,
        } => {
            if *limit_kmh < 0 {
                format!("前方{}，限速未知", dist_zh(*distance_m))
            } else if *limit_kmh == 0 {
                format!("前方{}，解除限速", dist_zh(*distance_m))
            } else {
                format!("前方{}，限速{}", dist_zh(*distance_m), limit_kmh)
            }
        }
        ReminderEvent::OverSpeed { limit_kmh } => {
            format!("您已超速，当前限速{}", limit_kmh)
        }
        ReminderEvent::RedLight { distance_m } => {
            format!("前方{}，红灯，请减速", dist_zh(*distance_m))
        }
        ReminderEvent::GreenImminent => "即将绿灯".to_string(),
        ReminderEvent::Glosa {
            v_min_kmh,
            v_max_kmh,
        } => {
            format!("建议保持{}到{}", v_min_kmh, v_max_kmh)
        }
    }
}

/// 距离的中文播报（300→"3百米"、250→"250米"——整百用百米简化，避免"米米"重复）。
fn dist_zh(m: u32) -> String {
    if m.is_multiple_of(100) {
        format!("{}百米", m / 100)
    } else {
        format!("{m}米")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speak_distance_standard() {
        // 城市 50 km/h local：50×10=500 → clamp 500 ×0.5 = 250m
        let cfg = SpeakConfig::default();
        assert_eq!(speak_ahead_distance_m(50.0, 1, 0, &cfg), 250);
        // 高速 90 km/h motorway：900 ×1.0 = 900m
        assert_eq!(speak_ahead_distance_m(90.0, 3, 0, &cfg), 900);
        // 快速路 80 express：800×0.8=640
        assert_eq!(speak_ahead_distance_m(80.0, 2, 0, &cfg), 640);
    }

    #[test]
    fn speak_distance_clamp_and_complexity() {
        let cfg = SpeakConfig::default();
        // 20 km/h 低速 clamp 下限：200×0.5=100
        assert_eq!(speak_ahead_distance_m(20.0, 1, 0, &cfg), 100);
        // 130 km/h：1300×1.0=1300（clamp 上限 2000 不触发）
        assert_eq!(speak_ahead_distance_m(130.0, 3, 0, &cfg), 1300);
        // 200 km/h：2000×1.0→clamp 2000
        assert_eq!(speak_ahead_distance_m(200.0, 3, 0, &cfg), 2000);
        // 环岛复杂度 3+：250×1.3=325
        assert_eq!(speak_ahead_distance_m(50.0, 1, 4, &cfg), 325);
    }

    #[test]
    fn speak_distance_frequency_scale() {
        let low = SpeakConfig {
            frequency: FrequencyProfile::Low,
            ..Default::default()
        };
        let high = SpeakConfig {
            frequency: FrequencyProfile::High,
            ..Default::default()
        };
        assert_eq!(speak_ahead_distance_m(90.0, 3, 0, &low), 450);
        assert_eq!(speak_ahead_distance_m(90.0, 3, 0, &high), 1350);
    }

    #[test]
    fn reminder_gate_interval() {
        let mut gate = ReminderGate::new();
        let cfg = SpeakConfig::default();
        assert!(gate.allow("overspeed", 0.0, &cfg));
        assert!(!gate.allow("overspeed", 10.0, &cfg)); // 20s 内拒绝
        assert!(gate.allow("overspeed", 31.0, &cfg)); // 30s 后放行
                                                      // 不同类型互不影响
        assert!(gate.allow("redlight", 5.0, &cfg));
    }

    #[test]
    fn speech_zh_texts() {
        assert_eq!(
            to_speech_zh(&ReminderEvent::SpeedLimitChange {
                distance_m: 300,
                limit_kmh: 50
            }),
            "前方3百米，限速50"
        );
        assert_eq!(
            to_speech_zh(&ReminderEvent::SpeedLimitChange {
                distance_m: 500,
                limit_kmh: 0
            }),
            "前方5百米，解除限速"
        );
        assert_eq!(
            to_speech_zh(&ReminderEvent::SpeedLimitChange {
                distance_m: 500,
                limit_kmh: -1
            }),
            "前方5百米，限速未知"
        );
        assert_eq!(
            to_speech_zh(&ReminderEvent::OverSpeed { limit_kmh: 50 }),
            "您已超速，当前限速50"
        );
        assert_eq!(
            to_speech_zh(&ReminderEvent::RedLight { distance_m: 250 }),
            "前方250米，红灯，请减速"
        );
        assert_eq!(to_speech_zh(&ReminderEvent::GreenImminent), "即将绿灯");
        assert_eq!(
            to_speech_zh(&ReminderEvent::Glosa {
                v_min_kmh: 50,
                v_max_kmh: 60
            }),
            "建议保持50到60"
        );
    }
}
