// P2-17 Navigation Session（§118-121）。
// 中央状态机：Idle/DestinationSet/Planning/Navigating/SuspectedOffRoute/Rerouting/
// Arrived/Paused/LostPosition/Error；协调 Telemetry + MapMatcher + Router + RouteTracker
// + ManeuverGenerator + SignalLinker；NavigationSnapshot 统一输出（headless，无 UI）。
use nav_graph::CompactGraph;
use nav_spatial::SpatialIndex;
use nav_telemetry::TelemetrySnapshot;

use crate::cost::RouteProfile;
use crate::destination::Destination;
use crate::maneuver::{generate_maneuvers, Maneuver, TurnLookup};
use crate::reroute::{OffRouteState, RerouteConfig, RerouteDetector};
use crate::search::{Route, RouteRequest, Router};
use crate::signal::{SignalLinker, UpcomingSignal};
use crate::snap::{snap_nearest, VirtualEndpoint};
use crate::tracker::RouteTracker;

/// 会话状态（§118）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    DestinationSet,
    Planning,
    Navigating,
    SuspectedOffRoute,
    Rerouting,
    Arrived,
    Paused,
    LostPosition,
    Error,
}

impl SessionState {
    pub fn name(&self) -> &'static str {
        match self {
            SessionState::Idle => "Idle",
            SessionState::DestinationSet => "DestinationSet",
            SessionState::Planning => "Planning",
            SessionState::Navigating => "Navigating",
            SessionState::SuspectedOffRoute => "SuspectedOffRoute",
            SessionState::Rerouting => "Rerouting",
            SessionState::Arrived => "Arrived",
            SessionState::Paused => "Paused",
            SessionState::LostPosition => "LostPosition",
            SessionState::Error => "Error",
        }
    }
}

/// 统一输出（§120）。
#[derive(Debug, Clone)]
pub struct NavigationSnapshot {
    pub state: SessionState,
    pub position: Option<(f64, f64, f64)>,
    pub matched_edge: Option<u32>,
    pub match_confidence: Option<nav_matcher::MatchConfidence>,
    pub route_distance_m: Option<f64>,
    pub remaining_m: Option<f64>,
    pub remaining_s: Option<f64>,
    pub progress: Option<f64>,
    pub next_maneuver: Option<Maneuver>,
    pub upcoming_signal: Option<UpcomingSignal>,
    pub destination: Option<String>,
    pub diagnostics: String,
}

/// 会话配置。
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub profile: RouteProfile,
    pub matcher: nav_matcher::MatcherConfig,
    pub reroute: RerouteConfig,
    /// 到达判定：剩余距离阈值（米）。
    pub arrive_margin_m: f64,
    /// 暂停判定：连续低速度帧数（§118 Paused）。
    pub pause_frames: u32,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            profile: RouteProfile::Fastest,
            matcher: nav_matcher::MatcherConfig::default(),
            reroute: RerouteConfig::default(),
            arrive_margin_m: 30.0,
            pause_frames: 600, // ~30s @ 20Hz
        }
    }
}

/// 导航会话（headless 状态机）。
pub struct NavigationSession {
    graph: std::rc::Rc<CompactGraph>,
    spatial: std::rc::Rc<SpatialIndex>,
    turns: TurnLookup,
    cfg: SessionConfig,
    state: SessionState,
    matcher: nav_matcher::MapMatcher,
    router: Router,
    detector: RerouteDetector,
    tracker: Option<RouteTracker>,
    route: Option<Route>,
    destination: Option<Destination>,
    low_speed_frames: u32,
    last_position: Option<(f64, f64, f64)>,
    frames: u64,
}

impl NavigationSession {
    pub fn new(
        graph: std::rc::Rc<CompactGraph>,
        spatial: std::rc::Rc<SpatialIndex>,
        turns: TurnLookup,
        cfg: SessionConfig,
    ) -> Self {
        let matcher = nav_matcher::MapMatcher::new(cfg.matcher.clone());
        let router = Router::new(graph.node_count());
        let detector = RerouteDetector::new(cfg.reroute.clone());
        NavigationSession {
            graph,
            spatial,
            turns,
            cfg,
            state: SessionState::Idle,
            matcher,
            router,
            detector,
            tracker: None,
            route: None,
            destination: None,
            low_speed_frames: 0,
            last_position: None,
            frames: 0,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    /// 当前路线（P2-18 Debug CLI 用）。
    pub fn route(&self) -> Option<&Route> {
        self.route.as_ref()
    }

    /// 图引用。
    pub fn graph(&self) -> &CompactGraph {
        &self.graph
    }

    /// 设置目的地（§107 统一模型）：→ Planning → Navigating（同步规划）。
    pub fn set_destination(&mut self, dest: Destination) -> Result<(), String> {
        self.destination = Some(dest);
        self.state = SessionState::Planning;
        self.plan()
    }

    /// 规划（§91 语义：当前 snap + destination + profile）。
    fn plan(&mut self) -> Result<(), String> {
        let Some(dest) = &self.destination else {
            self.state = SessionState::Error;
            return Err("无目的地".into());
        };
        // 起点：当前匹配位置（无匹配则 snap 当前位置）
        let start_snap = self
            .last_position
            .and_then(|(x, _y, z)| snap_nearest(&self.graph, &self.spatial, x, z, 300.0))
            .ok_or_else(|| {
                self.state = SessionState::Error;
                "无法确定起点".to_string()
            })?;
        let req = RouteRequest::new(
            &self.graph,
            VirtualEndpoint::start(&start_snap, true),
            VirtualEndpoint::goal(&dest.access_snap),
            self.cfg.profile,
        );
        match self.router.astar(&req) {
            Some(route) => {
                if cfg!(test) {
                    eprintln!(
                        "DEBUG plan: start_edge={} goal_edge={} edges={:?} dist={}",
                        req.start.edge_id, req.goal.edge_id, route.edges, route.distance_m
                    );
                }
                self.route = Some(route.clone());
                self.tracker = Some(RouteTracker::new(&self.graph, route, 4));
                self.detector.reset();
                self.state = SessionState::Navigating;
                Ok(())
            }
            None => {
                self.state = SessionState::Error;
                Err("规划失败".into())
            }
        }
    }

    /// 每帧更新（§119 协调：telemetry → matcher → tracker → off-route → 信号/maneuver）。
    pub fn on_frame(&mut self, snap: &TelemetrySnapshot) -> NavigationSnapshot {
        self.frames += 1;
        let pos = (snap.position[0], snap.position[1], snap.position[2]);
        self.last_position = Some(pos);
        // 暂停检测（低速持续）
        if snap.speed < 0.5 {
            self.low_speed_frames += 1;
        } else {
            self.low_speed_frames = 0;
        }
        let mut diag = String::new();
        // —— Map Matching ——
        let yaw = quat_yaw(snap.heading);
        let mm = self
            .matcher
            .match_frame(&self.graph, &self.spatial, pos.0, pos.2, yaw);
        // 帧间位移（off-route 距离证据 §89）
        let frame_dist = self
            .last_position
            .map(|(lx, _ly, lz)| ((pos.0 - lx).powi(2) + (pos.2 - lz).powi(2)).sqrt())
            .unwrap_or(0.0);
        // —— 状态机 ——
        match self.state {
            SessionState::Idle | SessionState::DestinationSet => {}
            SessionState::Planning => {
                let _ = self.plan();
            }
            SessionState::Navigating
            | SessionState::SuspectedOffRoute
            | SessionState::LostPosition => {
                if let Some(tracker) = self.tracker.as_mut() {
                    let pu = tracker.update(&self.graph, mm.edge_id, mm.offset);
                    let matched =
                        mm.confidence != nav_matcher::MatchConfidence::Unmatched && pu.matched;
                    let st = self.detector.on_frame(matched, frame_dist);
                    match st {
                        OffRouteState::OnRoute => {
                            self.state = SessionState::Navigating;
                            // 到达判定（用 update 的剩余——含虚拟段路线语义）
                            if pu.remaining_distance < self.cfg.arrive_margin_m {
                                self.state = SessionState::Arrived;
                            }
                        }
                        OffRouteState::SuspectedOffRoute => {
                            self.state = SessionState::SuspectedOffRoute;
                        }
                        OffRouteState::OffRoute => {
                            // §91：确认偏航 → 重规划（同步；§92 worker 线程语义由调用方决定）
                            self.state = SessionState::Rerouting;
                            self.detector.begin_rerouting();
                        }
                        OffRouteState::Rerouting => {}
                    }
                }
            }
            SessionState::Rerouting => {
                // 重规划
                match self.plan() {
                    Ok(()) => {
                        self.detector.reset();
                        self.state = SessionState::Navigating;
                        diag = "rerouted".into();
                    }
                    Err(_) => {
                        self.state = SessionState::Error;
                        diag = "reroute failed".into();
                    }
                }
            }
            SessionState::Arrived | SessionState::Paused | SessionState::Error => {}
        }
        // 暂停判定
        if self.state == SessionState::Navigating && self.low_speed_frames >= self.cfg.pause_frames
        {
            self.state = SessionState::Paused;
        }
        // —— 输出组装 ——
        let progress = self.tracker.as_ref().map(|t| t.progress());
        let remaining_m = self.tracker.as_ref().map(|t| t.edge_remaining(&self.graph));
        let remaining_s = self.tracker.as_ref().map(|t| t.time_remaining(&self.graph));
        let next_maneuver = self.next_maneuver();
        let upcoming_signal = self.next_signal();
        NavigationSnapshot {
            state: self.state,
            position: Some(pos),
            matched_edge: (mm.edge_id != u32::MAX).then_some(mm.edge_id),
            match_confidence: Some(mm.confidence),
            route_distance_m: self.route.as_ref().map(|r| r.distance_m),
            remaining_m,
            remaining_s,
            progress,
            next_maneuver,
            upcoming_signal,
            destination: self.destination.as_ref().map(|d| d.name.clone()),
            diagnostics: diag,
        }
    }

    /// 下一个 maneuver（当前 tracker 位置之后；§94-99）。
    fn next_maneuver(&self) -> Option<Maneuver> {
        let route = self.route.as_ref()?;
        let from = self.tracker.as_ref()?.edge_index();
        let ms = generate_maneuvers(&self.graph, route, &self.turns);
        // 找 route_edge_index > from 的第一个非 Continue 类型
        ms.into_iter()
            .find(|m| m.route_edge_index > from && m.mtype != crate::maneuver::ManeuverType::Depart)
    }

    /// 下一个受控信号（§112）。
    fn next_signal(&self) -> Option<UpcomingSignal> {
        let route = self.route.as_ref()?;
        let from = self.tracker.as_ref()?.edge_index();
        let (i, eid, juid, group) =
            SignalLinker::next_controlled_movement(&self.graph, route, from)?;
        let pose = SignalLinker::static_head_pose(&self.graph, eid);
        let lights = nav_telemetry::read_semaphores();
        let lights: Vec<crate::signal::RuntimeSignal> = match &lights {
            Some(l) => l
                .iter()
                .map(|s| crate::signal::RuntimeSignal {
                    position: s.position,
                    quat: s.quat,
                    kind: s.kind,
                    time_remaining: s.time_remaining,
                    state: s.state,
                    id: s.id,
                })
                .collect(),
            None => Vec::new(),
        };
        let mut up = SignalLinker::link(eid, juid, group, &pose, &lights);
        up.route_edge_index = i;
        Some(up)
    }
}

/// 四元数 → yaw（世界弧度；SCS quat (x,y,z,w) 绕 Y 轴——与 signal::light_yaw 一致，
/// 审查修复：原绕 Z 公式对纯 yaw quat 输出 0/180°）。
fn quat_yaw(q: [f32; 4]) -> f64 {
    let (x, y, z, w) = (q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64);
    (2.0 * (w * y - x * z)).atan2(1.0 - 2.0 * (y * y + z * z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::destination::DestKind;
    use nav_dataset::{Edge, EdgeKind, Node, RoutingGraph};

    /// 直线图（0→1→2），目的地节点 2。
    fn setup() -> (
        std::rc::Rc<CompactGraph>,
        std::rc::Rc<SpatialIndex>,
        Destination,
    ) {
        let nodes = vec![
            Node {
                uid: 1,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            Node {
                uid: 2,
                x: 1000.0,
                y: 0.0,
                z: 0.0,
            },
            Node {
                uid: 3,
                x: 2000.0,
                y: 0.0,
                z: 0.0,
            },
        ];
        let mk = |from: u32, to: u32| Edge {
            from,
            to,
            kind: EdgeKind::Road,
            length: 1.0,
            source_uid: 0,
            geometry: vec![
                (nodes[from as usize].x, 0.0, nodes[from as usize].z),
                (nodes[to as usize].x, 0.0, nodes[to as usize].z),
            ],
            speed_limit: 50,
            road_class: 1,
            semaphore_id: -1,
            flags: 0,
            movement_id: None,
        };
        let g = std::rc::Rc::new(CompactGraph::build(&RoutingGraph {
            nodes: nodes.clone(),
            edges: vec![mk(0, 1), mk(1, 2)],
        }));
        let sp = std::rc::Rc::new(SpatialIndex::build(&g, 256.0));
        let dest = Destination {
            kind: DestKind::Coordinate,
            name: "goal".into(),
            position: (2000.0, 0.0, 0.0),
            access_snap: crate::snap::snap_nearest(&g, &sp, 2000.0, 0.0, 100.0).unwrap(),
        };
        (g, sp, dest)
    }

    fn telemetry_at(x: f64, speed: f32) -> TelemetrySnapshot {
        TelemetrySnapshot {
            sequence: 1,
            layout_version: 1,
            running: true,
            paused: false,
            simulation_time: 0,
            paused_simulation_time: 0,
            render_time: 0,
            game_time_minutes: 0,
            local_scale: 1.0,
            rest_stop_minutes: 0,
            position: [x, 0.0, 0.0],
            heading: [0.0, 0.0, 0.0, 1.0],
            speed,
            speed_limit: 50.0,
            fuel_amount: 1.0,
            fuel_range: 1000.0,
            fuel_warning: false,
            job: None,
        }
    }

    #[test]
    fn session_reaches_arrived_along_route() {
        let (g, sp, dest) = setup();
        let mut s = NavigationSession::new(g, sp, TurnLookup::new(), SessionConfig::default());
        assert_eq!(s.state(), SessionState::Idle);
        s.on_frame(&telemetry_at(10.0, 14.0)); // 先确定位置（Idle 下匹配不导航）
        s.set_destination(dest).expect("规划应成功");
        assert_eq!(s.state(), SessionState::Navigating);
        // 沿路线行驶
        for x in (0..2000).step_by(50) {
            let snap = s.on_frame(&telemetry_at(x as f64 + 1.0, 14.0));
            assert_ne!(snap.state, SessionState::Error);
            if snap.state == SessionState::Arrived {
                return;
            }
        }
        // 终点帧
        let snap = s.on_frame(&telemetry_at(1999.0, 14.0));
        assert_eq!(
            snap.state,
            SessionState::Arrived,
            "应到达: {:?}",
            snap.state
        );
        assert!(snap.remaining_m.unwrap_or(9999.0) < 30.0);
    }

    #[test]
    fn session_detects_pause() {
        let (g, sp, dest) = setup();
        let mut s = NavigationSession::new(g, sp, TurnLookup::new(), SessionConfig::default());
        s.on_frame(&telemetry_at(10.0, 14.0));
        s.set_destination(dest).unwrap();
        // 低速 60 帧（< pause 600）不暂停
        for _ in 0..60 {
            s.on_frame(&telemetry_at(10.0, 0.1));
        }
        assert_eq!(s.state(), SessionState::Navigating);
        // 低速超过阈值 → Paused
        let cfg = SessionConfig {
            pause_frames: 50,
            ..SessionConfig::default()
        };
        let (g2, sp2, dest2) = setup();
        let mut s2 = NavigationSession::new(g2, sp2, TurnLookup::new(), cfg);
        s2.on_frame(&telemetry_at(10.0, 14.0));
        s2.set_destination(dest2).unwrap();
        for _ in 0..60 {
            s2.on_frame(&telemetry_at(10.0, 0.1));
        }
        assert_eq!(s2.state(), SessionState::Paused);
    }
}
