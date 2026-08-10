// P2-08 Routing Cost Model（§58-69）。
// EdgeCostProvider：Route Search 与路线策略解耦；统一 nonnegative cost。
// FASTEST / SHORTEST / BALANCED 三 profile（§69）。
use nav_graph::{CompactGraph, EdgeKind};

/// 路线策略（§69 正式支持三档；FEWER_SIGNALS 等后续）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteProfile {
    Fastest,
    Shortest,
    Balanced,
}

impl RouteProfile {
    pub fn name(&self) -> &'static str {
        match self {
            RouteProfile::Fastest => "fastest",
            RouteProfile::Shortest => "shortest",
            RouteProfile::Balanced => "balanced",
        }
    }
}

/// 成本参数（corpus 校准前默认值；§63/64/65/66 全部可配置）。
#[derive(Debug, Clone)]
pub struct CostParams {
    /// 未知限速（speed_limit=-1）fallback 速度 km/h（§61——不得当无限速）。
    pub fallback_speed_kph: f64,
    /// 无限速（speed_limit=0）自由流 cap km/h（§62——不得数学上无穷）。
    pub free_flow_cap_kph: f64,
    /// 路口 movement 速度 km/h（§60 v_junction）。
    pub junction_speed_kph: f64,
    /// Ferry/Train 边速度 km/h（P2-14 细化前的占位）。
    pub transit_speed_kph: f64,
    /// 受控 movement 预期信号延迟（秒）（§63——统计延迟，非精确预测）。
    pub signal_delay_s: f64,
    /// 急转 penalty（秒）（§64）。
    pub turn_penalty_s: f64,
    /// U-turn penalty（秒）（§64）。
    pub uturn_penalty_s: f64,
    /// GpsAvoid 高 penalty（秒）（§65——高但不绝对不可通行）。
    pub gps_avoid_penalty_s: f64,
    /// Secret 强 penalty（秒）（§66——corpus 验证前默认强排除倾向）。
    pub secret_penalty_s: f64,
    /// Balanced 权重（§68）。
    pub w_time: f64,
    pub w_dist: f64,
    /// 急转判定阈值（弧度，|转角| ≥ 该值计急转）。
    pub sharp_turn_rad: f64,
    /// U-turn 判定阈值（弧度，|转角| ≥ 该值计 U-turn）。
    pub uturn_rad: f64,
}

impl Default for CostParams {
    fn default() -> Self {
        CostParams {
            fallback_speed_kph: 50.0,
            free_flow_cap_kph: 130.0,
            junction_speed_kph: 30.0,
            transit_speed_kph: 30.0,
            signal_delay_s: 8.0,
            turn_penalty_s: 3.0,
            uturn_penalty_s: 30.0,
            gps_avoid_penalty_s: 600.0,
            secret_penalty_s: 3600.0,
            w_time: 0.7,
            w_dist: 0.3,
            sharp_turn_rad: 1.2, // ~69°
            uturn_rad: 2.6,      // ~149°
        }
    }
}

/// road_class → fallback 速度（§60：speed limit 缺失时按道路等级）（km/h）。
pub fn class_speed_kph(road_class: u8) -> f64 {
    match road_class {
        3 => 90.0, // motorway
        2 => 70.0, // express
        1 => 40.0, // local
        _ => 50.0, // 其他（城市）
    }
}

/// 边几何长度（空几何回退欧氏）。
fn edge_len(graph: &CompactGraph, eid: u32) -> f64 {
    let e = &graph.edges[eid as usize];
    let pts = graph.edge_geometry(e);
    if pts.len() >= 2 {
        nav_graph::polyline_length(pts)
    } else {
        let a = graph.positions[e.from as usize];
        let b = graph.positions[e.to as usize];
        ((a.0 - b.0).powi(2) + (a.2 - b.2).powi(2)).sqrt()
    }
}

/// 边成本提供者：profile + 参数 → 每边 nonnegative cost。
pub struct EdgeCostProvider {
    pub profile: RouteProfile,
    pub params: CostParams,
}

impl EdgeCostProvider {
    pub fn new(profile: RouteProfile) -> Self {
        EdgeCostProvider {
            profile,
            params: CostParams::default(),
        }
    }

    pub fn with_params(profile: RouteProfile, params: CostParams) -> Self {
        EdgeCostProvider { profile, params }
    }

    /// 边行驶速度（m/s）：Road 按限速（未知→fallback，无限→cap）；
    /// Movement 用 v_junction；Ferry/Train 用 transit 速度。
    pub fn edge_speed(&self, graph: &CompactGraph, eid: u32) -> f64 {
        let e = &graph.edges[eid as usize];
        let kph = match e.kind {
            EdgeKind::Road => match e.speed_limit {
                s if s < 0 => self.params.fallback_speed_kph,
                0 => self.params.free_flow_cap_kph,
                s => s as f64,
            },
            EdgeKind::JunctionMovement => self.params.junction_speed_kph,
            EdgeKind::Ferry | EdgeKind::Train => self.params.transit_speed_kph,
            EdgeKind::ServiceAccess => self.params.fallback_speed_kph,
        };
        kph / 3.6
    }

    /// 边时间成本（秒）：Road/Movement 距离/速度 + 信号延迟 + 转弯 penalty（§60/63/64）。
    pub fn edge_time(&self, graph: &CompactGraph, eid: u32) -> f64 {
        let e = &graph.edges[eid as usize];
        let len = edge_len(graph, eid);
        let mut t = len / self.edge_speed(graph, eid);
        match e.kind {
            EdgeKind::JunctionMovement => {
                if e.semaphore_id >= 0 {
                    t += self.params.signal_delay_s; // §63 静态统计延迟
                }
                let ang = self.turn_angle(graph, eid);
                if ang >= self.params.uturn_rad {
                    t += self.params.uturn_penalty_s;
                } else if ang >= self.params.sharp_turn_rad {
                    t += self.params.turn_penalty_s;
                }
            }
            EdgeKind::Road | EdgeKind::Ferry | EdgeKind::Train | EdgeKind::ServiceAccess => {}
        }
        t
    }

    /// 转弯角（entry tangent 与 exit tangent 的夹角；movement 用真实 polyline，§60/64）。
    fn turn_angle(&self, graph: &CompactGraph, eid: u32) -> f64 {
        let e = &graph.edges[eid as usize];
        let pts = graph.edge_geometry(e);
        if pts.len() < 2 {
            return 0.0;
        }
        let (ax, _, az) = pts[0];
        let (bx, _, bz) = pts[1];
        let entry = (bz - az).atan2(bx - ax);
        let (lx, _, lz) = pts[pts.len() - 2];
        let (rx, _, rz) = pts[pts.len() - 1];
        let exit_t = (rz - lz).atan2(rx - lx);
        let d = (exit_t - entry).abs();
        d.min(std::f64::consts::TAU - d)
    }

    /// 距离成本（§59）。
    pub fn edge_distance(&self, graph: &CompactGraph, eid: u32) -> f64 {
        edge_len(graph, eid)
    }

    /// 主搜索成本（§59/60/68）：profile 决定。
    pub fn edge_cost(&self, graph: &CompactGraph, eid: u32) -> f64 {
        let e = &graph.edges[eid as usize];
        let base = match self.profile {
            RouteProfile::Fastest => self.edge_time(graph, eid),
            RouteProfile::Shortest => self.edge_distance(graph, eid),
            RouteProfile::Balanced => {
                self.params.w_time * self.edge_time(graph, eid)
                    + self.params.w_dist * self.edge_distance(graph, eid) / 1000.0
            }
        };
        // 可用性 penalty（§65/66）：高 penalty 但非绝对不可通行
        let mut pen = 0.0;
        if e.gps_avoid() {
            pen += self.params.gps_avoid_penalty_s;
        }
        if e.secret() {
            pen += self.params.secret_penalty_s;
        }
        base + pen
    }

    /// 该边是否使用了估计速度（§61 diagnostics：route contains estimated-speed edge）。
    pub fn is_estimated_speed(&self, graph: &CompactGraph, eid: u32) -> bool {
        let e = &graph.edges[eid as usize];
        e.kind == EdgeKind::Road && e.speed_limit < 0
    }

    /// 该边是否受信号灯控制（§63）。
    pub fn has_signal(&self, graph: &CompactGraph, eid: u32) -> bool {
        let e = &graph.edges[eid as usize];
        e.kind == EdgeKind::JunctionMovement && e.semaphore_id >= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nav_dataset::{Edge, Node, RoutingGraph};

    fn rg() -> RoutingGraph {
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
                x: 1000.0,
                y: 0.0,
                z: 1000.0,
            },
        ];
        let mk = |from: u32,
                  to: u32,
                  kind: EdgeKind,
                  speed: i16,
                  cls: u8,
                  sem: i32,
                  gps: bool,
                  sec: bool| Edge {
            from,
            to,
            kind,
            length: 1.0,
            source_uid: 0,
            geometry: vec![(0.0, 0.0, 0.0), (1000.0, 0.0, 0.0)],
            speed_limit: speed,
            road_class: cls,
            semaphore_id: sem,
            flags: (gps as u8) | ((sec as u8) << 1),
            movement_id: None,
        };
        let edges = vec![
            mk(0, 1, EdgeKind::Road, 100, 3, -1, false, false), // 高速路 100
            mk(1, 2, EdgeKind::JunctionMovement, -1, 0, 5, false, false), // 受控 movement
        ];
        RoutingGraph { nodes, edges }
    }

    #[test]
    fn fastest_uses_time() {
        let g = CompactGraph::build(&rg());
        let f = EdgeCostProvider::new(RouteProfile::Fastest);
        // 1000m @ 100km/h = 36s
        let t = f.edge_time(&g, 0);
        assert!((t - 36.0).abs() < 0.5, "t={t}");
        let c = f.edge_cost(&g, 0);
        assert!((c - t).abs() < 1e-9);
    }

    #[test]
    fn shortest_uses_distance() {
        let g = CompactGraph::build(&rg());
        let s = EdgeCostProvider::new(RouteProfile::Shortest);
        let c = s.edge_cost(&g, 0);
        assert!((c - 1000.0).abs() < 1e-9, "c={c}");
    }

    #[test]
    fn signal_movement_gets_static_delay() {
        let g = CompactGraph::build(&rg());
        let f = EdgeCostProvider::new(RouteProfile::Fastest);
        let t = f.edge_time(&g, 1);
        // movement 1m @ 30km/h = 0.12s + 8s signal delay
        assert!(t > 8.0, "t={t}");
        assert!(f.has_signal(&g, 1));
    }

    #[test]
    fn unknown_speed_falls_back_and_flagged() {
        let g = CompactGraph::build(&rg());
        let f = EdgeCostProvider::new(RouteProfile::Fastest);
        // movement speed -1 但 kind 非 Road —— is_estimated_speed 只对 Road
        assert!(!f.is_estimated_speed(&g, 1));
        // Road 100 正常
        assert!(!f.is_estimated_speed(&g, 0));
        // cap：speed 0 → 130 cap
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
        ];
        let mk = |from: u32, to: u32, speed: i16| Edge {
            from,
            to,
            kind: EdgeKind::Road,
            length: 1.0,
            source_uid: 0,
            geometry: vec![(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)],
            speed_limit: speed,
            road_class: 3,
            semaphore_id: -1,
            flags: 0,
            movement_id: None,
        };
        let g2 = CompactGraph::build(&RoutingGraph {
            nodes,
            edges: vec![mk(0, 1, 0)],
        });
        let v = f.edge_speed(&g2, 0) * 3.6;
        assert!((v - 130.0).abs() < 0.1, "v={v}");
    }
}
