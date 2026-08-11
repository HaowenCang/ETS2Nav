// P3-01：前方限速查询（v0.2 §40）。
// 数据源实证（P3-00）：road item 无显式 speed_limit 属性，每 edge 单值限速
// （country × speed_class × IsCityRoad 模型，nav-graph speed_limit 字段）。
// 真实限速分段发生在相邻边之间——本模块沿 route edge 序列聚合
// (offset_m, limit) 断点，提供"前方 300 m 限速 50"式查询。零 schema 变更。

use crate::search::Route;
use nav_graph::{CompactEdge, CompactGraph, EdgeKind};

/// 限速断点：offset_m 为沿路线起点（含起点虚拟段偏移）累计距离，limit 单位 km/h
/// （-1 未知 / 0 无限速 / >0 限速）。-1 与 0 不与任何值合并，如实上报。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedBreak {
    pub offset_m: f32,
    pub limit: i16,
}

/// 沿 route 聚合前方限速断点（§40）。
///
/// - 相邻同值去重；-1/0 不与任何值合并（如实上报）；
/// - horizon_m 截断（超过后停止）；
/// - **虚拟段独立聚合**（P3-审计 B1 修复）：start_virtual/end_virtual 的边
///   不在 route.edges 序列中（search.rs 语义：edges 为完整边序列，虚拟段为
///   吸附边上的部分段）——先推 start_e 限速段（offset 0 起）并按 seg 推进，
///   再 edges 全序列（每边全长），最后 end_e 限速段（seg 后不再推进）；
/// - **JunctionMovement/Ferry/Train/ServiceAccess 边继承前值**（P3-01 实证：
///   写入端未定义这些边类型的限速语义，routing.graph 中 100% 为 -1；
///   路口内部短连接不产生限速变化，应继承所连接道路限速）；
/// - 无虚拟段且 edges 为空时返回空。
pub fn speed_breaks_ahead(route: &Route, graph: &CompactGraph, horizon_m: f32) -> Vec<SpeedBreak> {
    if route.edges.is_empty() && route.start_virtual.is_none() && route.end_virtual.is_none() {
        return Vec::new();
    }
    let mut out: Vec<SpeedBreak> = Vec::new();
    let mut acc: f32 = 0.0;
    let mut cur: Option<i16> = None;

    // 断点推进：限速变化时记录；超 horizon 停止。
    let advance = |e: &CompactEdge,
                   len: f32,
                   acc: &mut f32,
                   out: &mut Vec<SpeedBreak>,
                   cur: &mut Option<i16>| {
        if *acc >= horizon_m {
            return;
        }
        let limit = if e.kind == EdgeKind::Road {
            e.speed_limit
        } else {
            cur.unwrap_or(e.speed_limit)
        };
        if *cur != Some(limit) {
            out.push(SpeedBreak {
                offset_m: *acc,
                limit,
            });
            *cur = Some(limit);
        }
        *acc += len.max(0.0);
    };

    // 起点虚拟段（start_e ∉ edges）
    if let Some((eid, offset, forward)) = route.start_virtual {
        let e = &graph.edges[eid as usize];
        let seg = if forward {
            e.length - offset as f32
        } else {
            offset as f32
        };
        advance(e, seg, &mut acc, &mut out, &mut cur);
    }
    // edges 全序列（每边全长）
    for &eid in &route.edges {
        let e = &graph.edges[eid as usize];
        advance(e, e.length, &mut acc, &mut out, &mut cur);
    }
    // 终点虚拟段（end_e ∉ edges）
    if let Some((eid, _offset, _forward)) = route.end_virtual {
        if acc < horizon_m {
            let e = &graph.edges[eid as usize];
            let limit = if e.kind == EdgeKind::Road {
                e.speed_limit
            } else {
                cur.unwrap_or(e.speed_limit)
            };
            if cur != Some(limit) {
                out.push(SpeedBreak {
                    offset_m: acc,
                    limit,
                });
            }
        }
    }
    out
}

/// 路线前方 horizon 内的限速变化点数量（提醒决策用：限速变化提前量）。
pub fn speed_change_ahead(
    route: &Route,
    graph: &CompactGraph,
    horizon_m: f32,
) -> Option<SpeedBreak> {
    let breaks = speed_breaks_ahead(route, graph, horizon_m);
    // 首断点即起点限速本身，变化点从第 2 个起
    breaks.get(1).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nav_dataset::RoutingGraph;
    use nav_graph::{CompactGraph, EdgeKind};

    fn graph_with_limits(limits: &[i16]) -> (CompactGraph, Vec<u32>) {
        // 链式图：node i → i+1（limit per edge），每条 100m
        let rg = RoutingGraph {
            nodes: (0..=limits.len())
                .map(|i| nav_dataset::Node {
                    uid: i as u64,
                    x: i as f64 * 100.0,
                    y: 0.0,
                    z: 0.0,
                })
                .collect(),
            edges: limits
                .iter()
                .enumerate()
                .map(|(i, &lim)| nav_dataset::Edge {
                    from: i as u32,
                    to: i as u32 + 1,
                    kind: EdgeKind::Road,
                    length: 100.0,
                    source_uid: i as u64,
                    geometry: vec![],
                    speed_limit: lim,
                    road_class: 1,
                    semaphore_id: -1,
                    flags: 0,
                    movement_id: None,
                })
                .collect(),
        };
        let eids: Vec<u32> = (0..limits.len() as u32).collect();
        (CompactGraph::build(&rg), eids)
    }

    fn route_on(eids: &[u32]) -> Route {
        Route {
            profile: crate::cost::RouteProfile::Fastest,
            edges: eids.to_vec(),
            start_virtual: None,
            end_virtual: None,
            distance_m: eids.len() as f64 * 100.0,
            eta_s: 0.0,
            road_edge_count: eids.len() as u32,
            junction_count: 0,
            signal_count: 0,
            ferry_count: 0,
            train_count: 0,
            gps_avoid_distance: 0.0,
            unknown_speed_distance: 0.0,
        }
    }

    #[test]
    fn basic_breaks() {
        let (g, eids) = graph_with_limits(&[80, 50, 70]);
        let r = route_on(&eids);
        let breaks = speed_breaks_ahead(&r, &g, 1000.0);
        assert_eq!(
            breaks,
            vec![
                SpeedBreak {
                    offset_m: 0.0,
                    limit: 80
                },
                SpeedBreak {
                    offset_m: 100.0,
                    limit: 50
                },
                SpeedBreak {
                    offset_m: 200.0,
                    limit: 70
                },
            ]
        );
    }

    #[test]
    fn adjacent_same_merged() {
        let (g, eids) = graph_with_limits(&[80, 80, 50]);
        let r = route_on(&eids);
        let breaks = speed_breaks_ahead(&r, &g, 1000.0);
        assert_eq!(
            breaks,
            vec![
                SpeedBreak {
                    offset_m: 0.0,
                    limit: 80
                },
                SpeedBreak {
                    offset_m: 200.0,
                    limit: 50
                },
            ]
        );
    }

    #[test]
    fn horizon_truncates() {
        let (g, eids) = graph_with_limits(&[80, 50, 70, 30]);
        let r = route_on(&eids);
        // horizon 250m：覆盖 0-100（80→50 在 100）、100-200（50→70 在 200）、
        // 200-250（70 尚未变化）；30 的断点在 300m 之外不出现
        let breaks = speed_breaks_ahead(&r, &g, 250.0);
        assert_eq!(
            breaks,
            vec![
                SpeedBreak {
                    offset_m: 0.0,
                    limit: 80
                },
                SpeedBreak {
                    offset_m: 100.0,
                    limit: 50
                },
                SpeedBreak {
                    offset_m: 200.0,
                    limit: 70
                },
            ]
        );
    }

    #[test]
    fn unknown_and_unlimited_not_merged() {
        // -1（未知）与 0（无限速）各自成断点
        let (g, eids) = graph_with_limits(&[80, -1, 0, 80]);
        let r = route_on(&eids);
        let breaks = speed_breaks_ahead(&r, &g, 1000.0);
        assert_eq!(
            breaks,
            vec![
                SpeedBreak {
                    offset_m: 0.0,
                    limit: 80
                },
                SpeedBreak {
                    offset_m: 100.0,
                    limit: -1
                },
                SpeedBreak {
                    offset_m: 200.0,
                    limit: 0
                },
                SpeedBreak {
                    offset_m: 300.0,
                    limit: 80
                },
            ]
        );
    }

    #[test]
    fn start_forward_virtual_segment() {
        // 真实 Route 形态（审计 B1）：start_e 不在 edges 序列中。
        // start_e(100m, 80) offset 40 fwd → 虚拟段 60m（80）；edges[0](100m, 50) 全长
        let (g, eids) = graph_with_limits(&[80, 50]);
        let mut r = route_on(&eids[1..]); // edges 不含 start_e
        r.start_virtual = Some((eids[0], 40.0, true));
        let breaks = speed_breaks_ahead(&r, &g, 1000.0);
        assert_eq!(
            breaks,
            vec![
                SpeedBreak {
                    offset_m: 0.0,
                    limit: 80
                },
                SpeedBreak {
                    offset_m: 60.0,
                    limit: 50
                },
            ]
        );
    }

    #[test]
    fn start_backward_virtual_segment() {
        // backward：虚拟段 = [0, offset]（倒回边起点 40m），edges[0] 全长
        let (g, eids) = graph_with_limits(&[80, 50]);
        let mut r = route_on(&eids[1..]);
        r.start_virtual = Some((eids[0], 40.0, false));
        let breaks = speed_breaks_ahead(&r, &g, 1000.0);
        assert_eq!(
            breaks,
            vec![
                SpeedBreak {
                    offset_m: 0.0,
                    limit: 80
                },
                SpeedBreak {
                    offset_m: 40.0,
                    limit: 50
                },
            ]
        );
    }

    #[test]
    fn end_virtual_segment_break() {
        // edges: [100m 80]；end_e(60m, 50) fwd offset 30 → seg=30
        // 终点限速与 edges 末值不同 → 断点产生在 edges 总长 100m 处
        let (g, eids) = graph_with_limits(&[80, 50]);
        let mut r = route_on(&eids[..1]);
        r.end_virtual = Some((eids[1], 30.0, true));
        let breaks = speed_breaks_ahead(&r, &g, 1000.0);
        assert_eq!(
            breaks,
            vec![
                SpeedBreak {
                    offset_m: 0.0,
                    limit: 80
                },
                SpeedBreak {
                    offset_m: 100.0,
                    limit: 50
                },
            ]
        );
    }

    #[test]
    fn movement_edges_inherit_previous_limit() {
        // Road 80 → Movement(-1) → Road 50：movement 继承 80，不产生断点
        let rg = RoutingGraph {
            nodes: (0..4)
                .map(|i| nav_dataset::Node {
                    uid: i as u64,
                    x: i as f64 * 50.0,
                    y: 0.0,
                    z: 0.0,
                })
                .collect(),
            edges: vec![
                nav_dataset::Edge {
                    from: 0,
                    to: 1,
                    kind: EdgeKind::Road,
                    length: 100.0,
                    source_uid: 0,
                    geometry: vec![],
                    speed_limit: 80,
                    road_class: 1,
                    semaphore_id: -1,
                    flags: 0,
                    movement_id: None,
                },
                nav_dataset::Edge {
                    from: 1,
                    to: 2,
                    kind: EdgeKind::JunctionMovement,
                    length: 20.0,
                    source_uid: 1,
                    geometry: vec![],
                    speed_limit: -1,
                    road_class: 0,
                    semaphore_id: -1,
                    flags: 0,
                    movement_id: Some(1),
                },
                nav_dataset::Edge {
                    from: 2,
                    to: 3,
                    kind: EdgeKind::Road,
                    length: 100.0,
                    source_uid: 2,
                    geometry: vec![],
                    speed_limit: 50,
                    road_class: 1,
                    semaphore_id: -1,
                    flags: 0,
                    movement_id: None,
                },
            ],
        };
        let eids: Vec<u32> = (0..3).collect();
        let g = CompactGraph::build(&rg);
        let r = route_on(&eids);
        let breaks = speed_breaks_ahead(&r, &g, 1000.0);
        assert_eq!(
            breaks,
            vec![
                SpeedBreak {
                    offset_m: 0.0,
                    limit: 80
                },
                SpeedBreak {
                    offset_m: 120.0,
                    limit: 50
                }, // 20m movement 段无断点
            ]
        );
    }

    #[test]
    fn empty_route() {
        let g = CompactGraph::build(&RoutingGraph {
            nodes: vec![],
            edges: vec![],
        });
        let r = route_on(&[]);
        assert!(speed_breaks_ahead(&r, &g, 1000.0).is_empty());
    }
}
