// P2-13 Maneuver Generator（§94-99）：
// Route Edge Sequence → Maneuver[]。movement TurnType 优先（§96）+ 真实 polyline
// entry/exit tangent 几何细化 slight/normal/sharp（§97）+ 抑制规则（§98）+
// 高速分叉 Keep Left/Right（§99）+ Enter/ExitMotorway + Ferry/Train leg 标记（§105/106）。
use nav_graph::CompactGraph;
use std::collections::HashMap;

use crate::search::Route;

/// Maneuver 类型（§95 V1 集合）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManeuverType {
    Depart,
    Continue,
    SlightLeft,
    Left,
    SharpLeft,
    SlightRight,
    Right,
    SharpRight,
    KeepLeft,
    KeepRight,
    UTurn,
    EnterMotorway,
    ExitMotorway,
    Roundabout,
    Ferry,
    Train,
    Arrive,
}

impl ManeuverType {
    pub fn name(&self) -> &'static str {
        match self {
            ManeuverType::Depart => "Depart",
            ManeuverType::Continue => "Continue",
            ManeuverType::SlightLeft => "SlightLeft",
            ManeuverType::Left => "Left",
            ManeuverType::SharpLeft => "SharpLeft",
            ManeuverType::SlightRight => "SlightRight",
            ManeuverType::Right => "Right",
            ManeuverType::SharpRight => "SharpRight",
            ManeuverType::KeepLeft => "KeepLeft",
            ManeuverType::KeepRight => "KeepRight",
            ManeuverType::UTurn => "UTurn",
            ManeuverType::EnterMotorway => "EnterMotorway",
            ManeuverType::ExitMotorway => "ExitMotorway",
            ManeuverType::Roundabout => "Roundabout",
            ManeuverType::Ferry => "Ferry",
            ManeuverType::Train => "Train",
            ManeuverType::Arrive => "Arrive",
        }
    }
}

/// 单条 maneuver。
#[derive(Debug, Clone)]
pub struct Maneuver {
    pub mtype: ManeuverType,
    /// 触发位置的 route 边索引（Arrive = 最后一条）。
    pub route_edge_index: usize,
    pub position: (f64, f64, f64),
    /// 有符号转向角（弧度，正 = 右转；几何细化后的值）。
    pub bearing_change: f64,
    /// dataset TurnType（-1 左 / 0 直 / 1 右 / 2 U；无 movement 数据时 None）。
    pub turn_type: Option<i8>,
    /// 距上一 maneuver 的距离（米）。
    pub distance_from_prev: f64,
    /// 环岛出口编号（P2-14 填充前为 None）。
    pub roundabout_exit: Option<u32>,
}

/// junction turn 查询表：(junction_uid, movement_id) → TurnType。
pub type TurnLookup = HashMap<(u64, u32), i8>;

/// 生成 maneuver 序列。
pub fn generate_maneuvers(
    graph: &CompactGraph,
    route: &Route,
    turns: &TurnLookup,
) -> Vec<Maneuver> {
    let mut out: Vec<Maneuver> = Vec::new();
    if route.edges.is_empty() {
        // 单段路线：Depart + Arrive
        if let Some((eid, _, _)) = route.start_virtual.or(route.end_virtual) {
            let e = &graph.edges[eid as usize];
            let pts = graph.edge_geometry(e);
            if !pts.is_empty() {
                let p = pts[0];
                out.push(Maneuver {
                    mtype: ManeuverType::Depart,
                    route_edge_index: 0,
                    position: (p.0, p.1, p.2),
                    bearing_change: 0.0,
                    turn_type: None,
                    distance_from_prev: 0.0,
                    roundabout_exit: None,
                });
                let last = pts[pts.len() - 1];
                out.push(Maneuver {
                    mtype: ManeuverType::Arrive,
                    route_edge_index: 0,
                    position: (last.0, last.1, last.2),
                    bearing_change: 0.0,
                    turn_type: None,
                    distance_from_prev: 0.0,
                    roundabout_exit: None,
                });
            }
        }
        return out;
    }
    let n = route.edges.len();
    // Depart：第一条边起点
    out.push(Maneuver {
        mtype: ManeuverType::Depart,
        route_edge_index: 0,
        position: edge_entry(graph, route.edges[0]),
        bearing_change: 0.0,
        turn_type: None,
        distance_from_prev: 0.0,
        roundabout_exit: None,
    });
    let mut prev_dist = 0.0;
    let mut prev_geom_dist = 0.0;
    for i in 1..n {
        let eid = route.edges[i];
        let prev_eid = route.edges[i - 1];
        let e = &graph.edges[eid as usize];
        let dist = edge_len(graph, eid);
        // 距离累计（含上一 maneuver 后经过的边）
        prev_dist += prev_geom_dist;
        prev_geom_dist = dist;
        let pos = edge_entry(graph, eid);
        // leg 切换（§105/106）：ferry/train
        if e.kind == nav_graph::EdgeKind::Ferry {
            out.push(Maneuver {
                mtype: ManeuverType::Ferry,
                route_edge_index: i,
                position: pos,
                bearing_change: 0.0,
                turn_type: None,
                distance_from_prev: prev_dist,
                roundabout_exit: None,
            });
            prev_dist = 0.0;
            continue;
        }
        if e.kind == nav_graph::EdgeKind::Train {
            out.push(Maneuver {
                mtype: ManeuverType::Train,
                route_edge_index: i,
                position: pos,
                bearing_change: 0.0,
                turn_type: None,
                distance_from_prev: prev_dist,
                roundabout_exit: None,
            });
            prev_dist = 0.0;
            continue;
        }
        // 高速进入/退出（road_class 3 = motorway，§99）
        let prev_e = &graph.edges[prev_eid as usize];
        let entering = prev_e.road_class < 3 && e.road_class == 3;
        let exiting = prev_e.road_class == 3 && e.road_class < 3;
        if entering {
            out.push(Maneuver {
                mtype: ManeuverType::EnterMotorway,
                route_edge_index: i,
                position: pos,
                bearing_change: 0.0,
                turn_type: None,
                distance_from_prev: prev_dist,
                roundabout_exit: None,
            });
            prev_dist = 0.0;
            continue;
        }
        if exiting {
            out.push(Maneuver {
                mtype: ManeuverType::ExitMotorway,
                route_edge_index: i,
                position: pos,
                bearing_change: 0.0,
                turn_type: None,
                distance_from_prev: prev_dist,
                roundabout_exit: None,
            });
            prev_dist = 0.0;
            continue;
        }
        // 转向判定（§96 movement TurnType 优先 + §97 几何细化）
        let entry_tan = edge_exit_tangent(graph, prev_eid);
        let exit_tan = edge_entry_tangent(graph, eid);
        let mut signed = signed_angle(entry_tan, exit_tan); // 正 = 右
        let turn_type = if e.kind == nav_graph::EdgeKind::JunctionMovement {
            turns
                .get(&(e.source_uid, e.movement_id.unwrap_or(0)))
                .copied()
        } else {
            None
        };
        // 几何转角（实际绕行角——movement polyline 的 entry→exit tangent）
        let geom_angle = if e.kind == nav_graph::EdgeKind::JunctionMovement {
            let pts = graph.edge_geometry(e);
            if pts.len() >= 2 {
                let (ax, _, az) = pts[0];
                let (bx, _, bz) = pts[1];
                let en = (bz - az).atan2(bx - ax);
                let (lx, _, lz) = pts[pts.len() - 2];
                let (rx, _, rz) = pts[pts.len() - 1];
                let ex = (rz - lz).atan2(rx - lx);
                signed_angle(en, ex)
            } else {
                signed
            }
        } else {
            signed
        };
        signed = geom_angle;
        // 抑制（§98）：转角过小（自然弯曲）不生成 maneuver——除非 movement 语义明确转向
        let abs_d = signed.abs();
        let is_movement_turn = turn_type.is_some_and(|t| t != 0);
        if abs_d < 0.44 && !is_movement_turn {
            // 自然弯曲：合并为 Continue（不生成）
            continue;
        }
        let mtype = if turn_type == Some(2) {
            ManeuverType::UTurn
        } else if e.kind == nav_graph::EdgeKind::JunctionMovement && turn_type.is_some() {
            // §96：TurnType 优先（-1 左 / 1 右），几何细化 slight/normal/sharp（§97）
            match (turn_type.unwrap(), abs_d) {
                (-1, d) if d < 0.7 => ManeuverType::SlightLeft,
                (-1, d) if d < 1.6 => ManeuverType::Left,
                (-1, _) => ManeuverType::SharpLeft,
                (1, d) if d < 0.7 => ManeuverType::SlightRight,
                (1, d) if d < 1.6 => ManeuverType::Right,
                (1, _) => ManeuverType::SharpRight,
                _ => ManeuverType::Continue,
            }
        } else {
            // 纯几何（road 边）：分叉 Keep 判定（§99）
            let fork = fork_keep(graph, prev_eid, eid, entry_tan);
            match fork {
                Some(k) => k,
                None => classify_angle(signed),
            }
        };
        out.push(Maneuver {
            mtype,
            route_edge_index: i,
            position: pos,
            bearing_change: signed,
            turn_type,
            distance_from_prev: prev_dist,
            roundabout_exit: None,
        });
        prev_dist = 0.0;
    }
    // Arrive：最后一条边终点
    out.push(Maneuver {
        mtype: ManeuverType::Arrive,
        route_edge_index: n - 1,
        position: edge_exit(graph, route.edges[n - 1]),
        bearing_change: 0.0,
        turn_type: None,
        distance_from_prev: prev_dist,
        roundabout_exit: None,
    });
    out
}

/// 分叉 Keep 判定（§99）：当前节点多个出边且 route 走的是较偏者 → Keep 朝向。
fn fork_keep(
    graph: &CompactGraph,
    prev_eid: u32,
    cur_eid: u32,
    entry_tan: f64,
) -> Option<ManeuverType> {
    let prev_e = &graph.edges[prev_eid as usize];
    let cur_e = &graph.edges[cur_eid as usize];
    if cur_e.kind != nav_graph::EdgeKind::Road || prev_e.kind != nav_graph::EdgeKind::Road {
        return None;
    }
    let node = prev_e.to;
    // 出边集合（排除回边）
    let outs: Vec<u32> = graph
        .out_edges(node as usize)
        .iter()
        .copied()
        .filter(|&e| e != cur_eid && graph.edges[e as usize].kind == nav_graph::EdgeKind::Road)
        .collect();
    if outs.is_empty() {
        return None;
    }
    // 主路延续 = 与 entry_tan 最接近的出边方向
    let cur_tan = edge_entry_tangent(graph, cur_eid);
    let cur_dev = signed_angle(entry_tan, cur_tan).abs();
    // 最直出边
    let mut min_dev = f64::MAX;
    for &oe in &outs {
        let t = edge_entry_tangent(graph, oe);
        min_dev = min_dev.min(signed_angle(entry_tan, t).abs());
    }
    // 分叉存在（有更直的出边）且 route 偏转 < 44° → Keep（按偏转方向）
    if min_dev < 0.44 && (0.44..0.9).contains(&cur_dev) {
        return Some(if signed_angle(entry_tan, cur_tan) > 0.0 {
            ManeuverType::KeepRight
        } else {
            ManeuverType::KeepLeft
        });
    }
    None
}

/// 角度分类（几何：slight/normal/sharp + UTurn）。
fn classify_angle(signed: f64) -> ManeuverType {
    let d = signed.abs();
    if d >= 2.6 {
        return ManeuverType::UTurn;
    }
    let right = signed > 0.0;
    match d {
        x if x < 0.7 => {
            if right {
                ManeuverType::SlightRight
            } else {
                ManeuverType::SlightLeft
            }
        }
        x if x < 1.6 => {
            if right {
                ManeuverType::Right
            } else {
                ManeuverType::Left
            }
        }
        _ => {
            if right {
                ManeuverType::SharpRight
            } else {
                ManeuverType::SharpLeft
            }
        }
    }
}

/// 有符号角差（正 = 右转/顺时针）。
fn signed_angle(from: f64, to: f64) -> f64 {
    let mut d = (to - from) % std::f64::consts::TAU;
    if d > std::f64::consts::PI {
        d -= std::f64::consts::TAU;
    }
    if d < -std::f64::consts::PI {
        d += std::f64::consts::TAU;
    }
    d
}

/// 边起点（入口）位置。
fn edge_entry(g: &CompactGraph, eid: u32) -> (f64, f64, f64) {
    let e = &g.edges[eid as usize];
    let pts = g.edge_geometry(e);
    if pts.is_empty() {
        g.positions[e.from as usize]
    } else {
        pts[0]
    }
}

/// 边终点（出口）位置。
fn edge_exit(g: &CompactGraph, eid: u32) -> (f64, f64, f64) {
    let e = &g.edges[eid as usize];
    let pts = g.edge_geometry(e);
    if pts.is_empty() {
        g.positions[e.to as usize]
    } else {
        pts[pts.len() - 1]
    }
}

/// 边入口 tangent（第一段方向）。
fn edge_entry_tangent(g: &CompactGraph, eid: u32) -> f64 {
    let e = &g.edges[eid as usize];
    let pts = g.edge_geometry(e);
    if pts.len() >= 2 {
        (pts[1].2 - pts[0].2).atan2(pts[1].0 - pts[0].0)
    } else {
        let a = g.positions[e.from as usize];
        let b = g.positions[e.to as usize];
        (b.2 - a.2).atan2(b.0 - a.0)
    }
}

/// 边出口 tangent（最后一段方向）。
fn edge_exit_tangent(g: &CompactGraph, eid: u32) -> f64 {
    let e = &g.edges[eid as usize];
    let pts = g.edge_geometry(e);
    if pts.len() >= 2 {
        (pts[pts.len() - 1].2 - pts[pts.len() - 2].2)
            .atan2(pts[pts.len() - 1].0 - pts[pts.len() - 2].0)
    } else {
        let a = g.positions[e.from as usize];
        let b = g.positions[e.to as usize];
        (b.2 - a.2).atan2(b.0 - a.0)
    }
}

fn edge_len(g: &CompactGraph, eid: u32) -> f64 {
    let e = &g.edges[eid as usize];
    let pts = g.edge_geometry(e);
    if pts.len() >= 2 {
        nav_graph::polyline_length(pts)
    } else {
        let a = g.positions[e.from as usize];
        let b = g.positions[e.to as usize];
        ((a.0 - b.0).powi(2) + (a.2 - b.2).powi(2)).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::RouteProfile;
    use crate::search::Route;
    use nav_dataset::{Edge, EdgeKind, Node, RoutingGraph};

    /// 图：直路 → 右转 90° → 直路 → 高速（class3）→ 分叉（左 Keep）。
    fn graph() -> CompactGraph {
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
            Node {
                uid: 4,
                x: 2000.0,
                y: 0.0,
                z: 1000.0,
            },
            Node {
                uid: 5,
                x: 3000.0,
                y: 0.0,
                z: 1000.0,
            },
            Node {
                uid: 6,
                x: 2500.0,
                y: 0.0,
                z: 1200.0,
            },
        ];
        let mk = |from: u32, to: u32, cls: u8| Edge {
            from,
            to,
            kind: EdgeKind::Road,
            length: 1.0,
            source_uid: 0,
            geometry: vec![
                (nodes[from as usize].x, 0.0, nodes[from as usize].z),
                (nodes[to as usize].x, 0.0, nodes[to as usize].z),
            ],
            speed_limit: 60,
            road_class: cls,
            semaphore_id: -1,
            flags: 0,
            movement_id: None,
        };
        let edges = vec![
            mk(0, 1, 1), // 直路
            mk(1, 2, 1), // 右转 90°（z 增 = 北）
            mk(2, 3, 1), // 直路
            mk(3, 4, 3), // 进入高速
            mk(4, 5, 3), // 高速直行
            mk(4, 5, 3), // 分叉：同终点（第二出边，方向偏）
        ];
        CompactGraph::build(&RoutingGraph { nodes, edges })
    }

    fn route_of(g: &CompactGraph) -> Route {
        let edges: Vec<u32> = (0..6).collect();
        let dist: f64 = edges.iter().map(|&e| edge_len(g, e)).sum();
        Route {
            profile: RouteProfile::Fastest,
            edges,
            start_virtual: None,
            end_virtual: None,
            distance_m: dist,
            eta_s: 60.0,
            road_edge_count: 6,
            junction_count: 0,
            signal_count: 0,
            ferry_count: 0,
            train_count: 0,
            gps_avoid_distance: 0.0,
            unknown_speed_distance: 0.0,
        }
    }

    #[test]
    fn right_turn_and_motorway_maneuvers() {
        let g = graph();
        let route = route_of(&g);
        let turns = TurnLookup::new();
        let ms = generate_maneuvers(&g, &route, &turns);
        let names: Vec<&str> = ms.iter().map(|m| m.mtype.name()).collect();
        assert_eq!(names[0], "Depart");
        // 右转（90°）应生成 Right
        assert!(names.contains(&"Right"), "应含 Right: {names:?}");
        // 进入高速
        assert!(
            names.contains(&"EnterMotorway"),
            "应含 EnterMotorway: {names:?}"
        );
        assert_eq!(names[names.len() - 1], "Arrive");
    }

    #[test]
    fn gentle_curve_suppressed() {
        // 轻微弯曲（10°）不生成 maneuver
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
                z: 100.0,
            }, // 仅 5.7° 偏转
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
            speed_limit: 60,
            road_class: 1,
            semaphore_id: -1,
            flags: 0,
            movement_id: None,
        };
        let edges = vec![mk(0, 1), mk(1, 2)];
        let g = CompactGraph::build(&RoutingGraph { nodes, edges });
        let route = Route {
            profile: RouteProfile::Fastest,
            edges: vec![0, 1],
            start_virtual: None,
            end_virtual: None,
            distance_m: 2100.0,
            eta_s: 120.0,
            road_edge_count: 2,
            junction_count: 0,
            signal_count: 0,
            ferry_count: 0,
            train_count: 0,
            gps_avoid_distance: 0.0,
            unknown_speed_distance: 0.0,
        };
        let ms = generate_maneuvers(&g, &route, &TurnLookup::new());
        let names: Vec<&str> = ms.iter().map(|m| m.mtype.name()).collect();
        assert_eq!(names, vec!["Depart", "Arrive"], "轻微弯曲应抑制: {names:?}");
    }
}
