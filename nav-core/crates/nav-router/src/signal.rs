// P2-16 Signal Runtime Link（§111-117）。
// 桥接：JunctionMovement.SemaphoreId（P1 静态绑定）↔ runtime 灯槽（P0 semaphore-bridge）。
// 对 route 中下一个受控 movement：静态 signal head 姿态（§113 近似：movement polyline 入口端）
// + runtime 候选评分（§114：位置/方向/邻近度）→ UpcomingSignal + 置信度（§115 VERIFIED/PROBABLE/UNKNOWN）。
use nav_graph::CompactGraph;

use crate::search::Route;

/// runtime 灯槽（semaphore-bridge 48B/灯：pos 3f + quat 4f + type + time + state + id）。
#[derive(Debug, Clone, Copy)]
pub struct RuntimeSignal {
    pub position: (f64, f64, f64),
    pub quat: [f32; 4],
    pub kind: i32,
    pub time_remaining: f32,
    pub state: i32,
    pub id: i32,
}

/// 灯状态（P0 语义：1 红 / 2 绿 / 3 黄 等——is_valid_state 通过值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightState {
    Red = 1,
    Green = 2,
    Yellow = 3,
    Unknown = 0,
}

/// 关联置信度（§115：只有 VERIFIED 可交给 P3 做 countdown/GLOSA）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalConfidence {
    Verified,
    Probable,
    Unknown,
}

/// 路线中下一个受控信号（§112 输出模型）。
#[derive(Debug, Clone)]
pub struct UpcomingSignal {
    /// 受控 movement 的 route 边索引。
    pub route_edge_index: usize,
    pub movement_edge_id: u32,
    pub junction_uid: u64,
    /// PPD signal group（§116：不推断 major/minor）。
    pub semaphore_group: i32,
    /// 关联到的 runtime 灯（None = 未关联）。
    pub runtime: Option<RuntimeSignal>,
    pub state: Option<LightState>,
    pub remaining_time: Option<f64>,
    pub confidence: SignalConfidence,
}

/// 静态 signal head 姿态（§113：dataset v2 未存灯头几何——V2-6 近似：
/// movement polyline 入口端位置 + 入口 tangent 方向）。
#[derive(Debug, Clone, Copy)]
pub struct StaticHeadPose {
    pub position: (f64, f64, f64),
    pub heading: f64,
}

/// Signal Linker（纯算法——runtime 数据由调用方提供）。
pub struct SignalLinker;

impl SignalLinker {
    /// 路线中下一个受控 movement（从 route_edge_index 起；§111）。
    /// 返回 (route 边索引, movement_edge_id, junction_uid, semaphore_group)。
    pub fn next_controlled_movement(
        graph: &CompactGraph,
        route: &Route,
        from_edge_index: usize,
    ) -> Option<(usize, u32, u64, i32)> {
        for i in from_edge_index..route.edges.len() {
            let eid = route.edges[i];
            let e = &graph.edges[eid as usize];
            if e.kind == nav_graph::EdgeKind::JunctionMovement && e.semaphore_id >= 0 {
                return Some((i, eid, e.source_uid, e.semaphore_id));
            }
        }
        None
    }

    /// 静态 signal head 姿态（§113 近似）。
    pub fn static_head_pose(graph: &CompactGraph, movement_edge_id: u32) -> StaticHeadPose {
        let e = &graph.edges[movement_edge_id as usize];
        let pts = graph.edge_geometry(e);
        if pts.len() >= 2 {
            let (ax, _, az) = pts[0];
            let (bx, _, bz) = pts[1];
            StaticHeadPose {
                position: (ax, 0.0, az),
                heading: (bz - az).atan2(bx - ax),
            }
        } else {
            let a = graph.positions[e.from as usize];
            let b = graph.positions[e.to as usize];
            StaticHeadPose {
                position: a,
                heading: (b.2 - a.2).atan2(b.0 - a.0),
            }
        }
    }

    /// 灯 quat → yaw（绕 Y 轴旋转提取；SCS quat (x,y,z,w)）。
    pub fn light_yaw(q: [f32; 4]) -> f64 {
        let (x, y, z, w) = (q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64);
        (2.0 * (w * y - x * z)).atan2(1.0 - 2.0 * (y * y + z * z))
    }

    /// 关联（§114 评分）：S = 0.5·Spos + 0.3·Shead + 0.2·Sid。
    /// Spos：静态 head 与灯位置距离（15m 内高分线性衰减）；
    /// Shead：head 方向 vs 灯 yaw 差（0.9 rad 内高分）；
    /// Sid：灯 id == semaphore_group → 1，否则 0。
    /// VERIFIED：S ≥ 0.8 且 id 匹配；PROBABLE：S ≥ 0.55；否则 UNKNOWN。
    pub fn link(
        movement_edge_id: u32,
        junction_uid: u64,
        semaphore_group: i32,
        static_pose: &StaticHeadPose,
        runtime_lights: &[RuntimeSignal],
    ) -> UpcomingSignal {
        let mut best: Option<(f64, RuntimeSignal)> = None;
        for l in runtime_lights {
            let dx = l.position.0 - static_pose.position.0;
            let dz = l.position.2 - static_pose.position.2;
            let dist = (dx * dx + dz * dz).sqrt();
            let s_pos = (1.0 - dist / 15.0).clamp(0.0, 1.0);
            if s_pos <= 0.0 {
                continue; // 距离过远不参与（§114：只匹配当前车辆附近）
            }
            let ly = Self::light_yaw(l.quat);
            let d_ang = (ly - static_pose.heading).abs() % std::f64::consts::TAU;
            let d_ang = d_ang.min(std::f64::consts::TAU - d_ang);
            let s_head = (1.0 - d_ang / 0.9).clamp(0.0, 1.0);
            let s_id = if l.id == semaphore_group { 1.0 } else { 0.0 };
            let s = 0.5 * s_pos + 0.3 * s_head + 0.2 * s_id;
            if best.as_ref().is_none_or(|(bs, _)| s > *bs) {
                best = Some((s, *l));
            }
        }
        match best {
            Some((score, l)) => {
                let confidence = if score >= 0.8 && l.id == semaphore_group {
                    SignalConfidence::Verified
                } else if score >= 0.55 {
                    SignalConfidence::Probable
                } else {
                    SignalConfidence::Unknown
                };
                UpcomingSignal {
                    route_edge_index: 0, // 调用方填充
                    movement_edge_id,
                    junction_uid,
                    semaphore_group,
                    runtime: Some(l),
                    state: Some(light_state(l.state)),
                    remaining_time: Some(l.time_remaining as f64),
                    confidence,
                }
            }
            None => UpcomingSignal {
                route_edge_index: 0,
                movement_edge_id,
                junction_uid,
                semaphore_group,
                runtime: None,
                state: None,
                remaining_time: None,
                confidence: SignalConfidence::Unknown,
            },
        }
    }
}

/// state i32 → LightState（P0 is_valid_state 语义：1/2/3 有效）。
pub fn light_state(state: i32) -> LightState {
    match state {
        1 => LightState::Red,
        2 => LightState::Green,
        3 => LightState::Yellow,
        _ => LightState::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::RouteProfile;
    use crate::search::Route;
    use nav_dataset::{Edge, EdgeKind, Node, RoutingGraph};

    fn graph_with_signal() -> (CompactGraph, Route) {
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
                z: 500.0,
            },
        ];
        let edges = vec![
            Edge {
                from: 0,
                to: 1,
                kind: EdgeKind::Road,
                length: 1000.0,
                source_uid: 10,
                geometry: vec![(0.0, 0.0, 0.0), (1000.0, 0.0, 0.0)],
                speed_limit: 50,
                road_class: 1,
                semaphore_id: -1,
                flags: 0,
                movement_id: None,
            },
            Edge {
                from: 1,
                to: 2,
                kind: EdgeKind::JunctionMovement,
                length: 500.0,
                source_uid: 99,
                geometry: vec![(1000.0, 0.0, 0.0), (1000.0, 0.0, 500.0)],
                speed_limit: 30,
                road_class: 0,
                semaphore_id: 17,
                flags: 8,
                movement_id: Some(3),
            },
        ];
        let g = CompactGraph::build(&RoutingGraph { nodes, edges });
        let route = Route {
            profile: RouteProfile::Fastest,
            edges: vec![0, 1],
            start_virtual: None,
            end_virtual: None,
            distance_m: 1500.0,
            eta_s: 100.0,
            road_edge_count: 1,
            junction_count: 1,
            signal_count: 1,
            ferry_count: 0,
            train_count: 0,
            gps_avoid_distance: 0.0,
            unknown_speed_distance: 0.0,
        };
        (g, route)
    }

    #[test]
    fn next_controlled_movement_found() {
        let (g, route) = graph_with_signal();
        let hit = SignalLinker::next_controlled_movement(&g, &route, 0).expect("应有受控 movement");
        assert_eq!(hit.1, 1, "movement 边");
        assert_eq!(hit.2, 99, "junction uid");
        assert_eq!(hit.3, 17, "semaphore group");
    }

    #[test]
    fn link_verified_by_id_and_position() {
        let (g, _route) = graph_with_signal();
        let pose = SignalLinker::static_head_pose(&g, 1);
        // 灯：位置在 (1000, 0) 附近、id 匹配 17
        let lights = [RuntimeSignal {
            position: (1003.0, 0.0, 2.0),
            quat: [
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            kind: 1,
            time_remaining: 5.0,
            state: 1,
            id: 17,
        }];
        let up = SignalLinker::link(1, 99, 17, &pose, &lights);
        assert_eq!(
            up.confidence,
            SignalConfidence::Verified,
            "id 匹配应 VERIFIED"
        );
        assert_eq!(up.state, Some(LightState::Red));
        assert_eq!(up.remaining_time, Some(5.0));
    }

    #[test]
    fn link_provable_without_id_match() {
        let (g, _route) = graph_with_signal();
        let pose = SignalLinker::static_head_pose(&g, 1);
        // 位置/方向近但 id 不匹配 → PROBABLE
        let lights = [RuntimeSignal {
            position: (1005.0, 0.0, 5.0),
            quat: [
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            kind: 1,
            time_remaining: 8.0,
            state: 2,
            id: 42,
        }];
        let up = SignalLinker::link(1, 99, 17, &pose, &lights);
        assert_eq!(up.confidence, SignalConfidence::Probable);
        assert_eq!(up.state, Some(LightState::Green));
    }

    #[test]
    fn link_unknown_when_far() {
        let (g, _route) = graph_with_signal();
        let pose = SignalLinker::static_head_pose(&g, 1);
        let lights = [RuntimeSignal {
            position: (1200.0, 0.0, 200.0), // 远处
            quat: [0.0, 0.0, 0.0, 1.0],
            kind: 1,
            time_remaining: 3.0,
            state: 3,
            id: 17,
        }];
        let up = SignalLinker::link(1, 99, 17, &pose, &lights);
        assert_eq!(up.confidence, SignalConfidence::Unknown);
        assert!(up.runtime.is_none());
    }
}
