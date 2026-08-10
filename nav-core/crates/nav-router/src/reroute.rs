// P2-12 Off-route Detection + Rerouting（§88-93）。
// 不能 matched_edge != route_edge 立即 reroute：多帧证据 + hysteresis（§88/89）；
// 四态状态机 ON_ROUTE/SUSPECTED/OFF_ROUTE/REROUTING（§90）；确认后同 snap+destination+profile 重规划（§91）。
use nav_graph::CompactGraph;

use crate::cost::RouteProfile;
use crate::search::{Route, RouteRequest, Router};
use crate::snap::{SnapPoint, VirtualEndpoint};

/// 偏航状态（§90）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffRouteState {
    OnRoute,
    SuspectedOffRoute,
    OffRoute,
    Rerouting,
}

impl OffRouteState {
    pub fn name(&self) -> &'static str {
        match self {
            OffRouteState::OnRoute => "ON_ROUTE",
            OffRouteState::SuspectedOffRoute => "SUSPECTED",
            OffRouteState::OffRoute => "OFF_ROUTE",
            OffRouteState::Rerouting => "REROUTING",
        }
    }
}

/// 偏航检测配置（§89 证据权重）。
#[derive(Debug, Clone)]
pub struct RerouteConfig {
    /// 连续 unmatched 帧数 → SUSPECTED。
    pub suspect_frames: u32,
    /// 连续 unmatched 帧数 → OFF_ROUTE。
    pub confirm_frames: u32,
    /// 沿非 route 边累计前进距离（米）→ OFF_ROUTE（速度不低时距离证据更可靠）。
    pub confirm_distance_m: f64,
    /// 每帧行驶距离下限：低于此视为静止（不计入偏航距离）。
    pub min_speed_dist_m: f64,
}

impl Default for RerouteConfig {
    fn default() -> Self {
        RerouteConfig {
            suspect_frames: 5,
            confirm_frames: 15,
            confirm_distance_m: 80.0,
            min_speed_dist_m: 0.5,
        }
    }
}

/// 偏航检测器（hysteresis，§88）。
pub struct RerouteDetector {
    cfg: RerouteConfig,
    state: OffRouteState,
    consecutive_unmatched: u32,
    off_route_distance: f64,
}

impl RerouteDetector {
    pub fn new(cfg: RerouteConfig) -> Self {
        RerouteDetector {
            cfg,
            state: OffRouteState::OnRoute,
            consecutive_unmatched: 0,
            off_route_distance: 0.0,
        }
    }

    pub fn state(&self) -> OffRouteState {
        self.state
    }

    /// 每帧更新（§89 证据：窗口外匹配 + 沿非 route 前进距离）。
    /// matched：本帧是否在 route 窗口内匹配（tracker 输出）。
    /// frame_dist：本帧位移（米，非 route 上的前进量）。
    pub fn on_frame(&mut self, matched: bool, frame_dist: f64) -> OffRouteState {
        match self.state {
            OffRouteState::Rerouting => {
                // 重规划完成由外部调 reset/on_route
            }
            OffRouteState::OnRoute | OffRouteState::SuspectedOffRoute | OffRouteState::OffRoute => {
                if matched {
                    // 恢复 route corridor（§89 recovery possibility）
                    self.consecutive_unmatched = 0;
                    self.off_route_distance = 0.0;
                    self.state = OffRouteState::OnRoute;
                } else {
                    self.consecutive_unmatched += 1;
                    if frame_dist >= self.cfg.min_speed_dist_m {
                        self.off_route_distance += frame_dist;
                    }
                    if self.state == OffRouteState::OnRoute
                        && self.consecutive_unmatched >= self.cfg.suspect_frames
                    {
                        self.state = OffRouteState::SuspectedOffRoute;
                    }
                    if self.consecutive_unmatched >= self.cfg.confirm_frames
                        || self.off_route_distance >= self.cfg.confirm_distance_m
                    {
                        self.state = OffRouteState::OffRoute;
                    }
                }
            }
        }
        self.state
    }

    /// 重规划启动（外部调用）。
    pub fn begin_rerouting(&mut self) {
        self.state = OffRouteState::Rerouting;
    }

    /// 新路线就绪：重置检测器（重新评估）。
    pub fn reset(&mut self) {
        self.state = OffRouteState::OnRoute;
        self.consecutive_unmatched = 0;
        self.off_route_distance = 0.0;
    }
}

/// 重规划：当前 snap + 同 destination + 同 profile → 新路线（§91）。
pub fn reroute(
    graph: &CompactGraph,
    router: &mut Router,
    current_snap: &SnapPoint,
    destination: &SnapPoint,
    profile: RouteProfile,
) -> Option<Route> {
    let req = RouteRequest::new(
        graph,
        VirtualEndpoint::start(current_snap, true),
        VirtualEndpoint::goal(destination),
        profile,
    );
    router.astar(&req)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_transitions_with_hysteresis() {
        let mut d = RerouteDetector::new(RerouteConfig::default());
        // 帧 1-4 unmatched → ON_ROUTE（< suspect 5）
        for _ in 0..4 {
            assert_eq!(d.on_frame(false, 10.0), OffRouteState::OnRoute);
        }
        // 第 5 帧 → SUSPECTED
        assert_eq!(d.on_frame(false, 10.0), OffRouteState::SuspectedOffRoute);
        // 恢复 → ON_ROUTE（consecutive 清零）
        assert_eq!(d.on_frame(true, 0.0), OffRouteState::OnRoute);
        // 再偏航：15 帧确认 → OFF_ROUTE
        let mut off = false;
        for _ in 0..20 {
            if d.on_frame(false, 5.0) == OffRouteState::OffRoute {
                off = true;
                break;
            }
        }
        assert!(off, "连续帧应确认 OFF_ROUTE");
    }

    #[test]
    fn distance_evidence_triggers_off_route() {
        let mut d = RerouteDetector::new(RerouteConfig::default());
        // 每帧 30m（速度不低）→ 3 帧累计 90m > 80m → OFF_ROUTE（无需 15 帧）
        let mut off = false;
        for _ in 0..4 {
            if d.on_frame(false, 30.0) == OffRouteState::OffRoute {
                off = true;
                break;
            }
        }
        assert!(off, "距离证据应触发 OFF_ROUTE");
    }

    #[test]
    fn slow_driving_does_not_accumulate_distance() {
        let mut d = RerouteDetector::new(RerouteConfig::default());
        // 慢速 14 帧（0.4m < min_speed 0.5m）→ 距离不累计；帧数 14 < 15 → 仅 SUSPECTED
        for _ in 0..14 {
            d.on_frame(false, 0.4);
        }
        assert_eq!(
            d.state(),
            OffRouteState::SuspectedOffRoute,
            "慢速不应 OFF_ROUTE"
        );
        assert!(d.off_route_distance.abs() < 1e-9, "慢速不应累计偏航距离");
        // 快速 3 帧（30m/帧）→ 距离证据触发 OFF_ROUTE
        let mut d2 = RerouteDetector::new(RerouteConfig::default());
        let mut off = false;
        for _ in 0..4 {
            if d2.on_frame(false, 30.0) == OffRouteState::OffRoute {
                off = true;
                break;
            }
        }
        assert!(off, "距离证据应触发 OFF_ROUTE");
    }
}
