// P3-01：前方限速查询（v0.2 §40）。
// 数据源实证（P3-00）：road item 无显式 speed_limit 属性，每 edge 单值限速
// （country × speed_class × IsCityRoad 模型，nav-graph speed_limit 字段）。
// 真实限速分段发生在相邻边之间——本模块沿 route edge 序列聚合
// (offset_m, limit) 断点，提供"前方 300 m 限速 50"式查询。零 schema 变更。

use crate::search::Route;
use nav_graph::{CompactGraph, EdgeKind};

/// 限速断点：offset_m 为沿路线起点（含起点虚拟段偏移）累计距离，limit 单位 km/h
/// （-1 未知 / 0 无限速 / >0 限速）。-1 与 0 不与任何值合并，如实上报。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedBreak {
    pub offset_m: f32,
    pub limit: i16,
}

/// 沿 route 聚合前方限速断点（§40）。
///
/// - 相邻同值去重（含虚拟段起始偏移截断后的首边）；
/// - horizon_m 截断（超过后停止）；
/// - 空 edges（纯虚拟段路线）返回空；
/// - 首边按 start_virtual 偏移截断，末边按 end_virtual 偏移截断；
/// - **JunctionMovement/Ferry/Train/ServiceAccess 边继承前值**（P3-01 实证：
///   写入端未定义这些边类型的限速语义，routing.graph 中 100% 为 -1；
///   路口内部短连接不产生限速变化，应继承所连接道路限速）。
pub fn speed_breaks_ahead(route: &Route, graph: &CompactGraph, horizon_m: f32) -> Vec<SpeedBreak> {
    if route.edges.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<SpeedBreak> = Vec::new();
    let mut acc: f32 = 0.0;
    let mut cur: Option<i16> = None;

    for (i, &eid) in route.edges.iter().enumerate() {
        let e = &graph.edges[eid as usize];
        // 首边扣除起点虚拟段已行驶长度；末边扣除终点虚拟段未行驶长度
        let mut len = e.length.max(0.0);
        if i == 0 {
            if let Some((_, off, forward)) = route.start_virtual {
                len -= if forward {
                    off as f32
                } else {
                    e.length - off as f32
                };
            }
        }
        if i + 1 == route.edges.len() {
            if let Some((_, off, forward)) = route.end_virtual {
                let rest = if forward {
                    e.length - off as f32
                } else {
                    off as f32
                };
                len -= (e.length - rest).max(0.0);
            }
        }
        let len = len.max(0.0);
        if acc >= horizon_m {
            break;
        }
        // 非 Road 边继承当前限速（不产生断点）
        let limit = if e.kind == EdgeKind::Road {
            e.speed_limit
        } else {
            cur.unwrap_or(-1)
        };
        if cur != Some(limit) {
            out.push(SpeedBreak {
                offset_m: acc,
                limit,
            });
            cur = Some(limit);
        }
        acc += len;
    }
    // horizon 后首个断点补记（若有变化）——由 acc >= horizon 提前 break 的
    // 语义保证：超出 horizon 的断点不产生。
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
    fn start_offset_shifts_first_break() {
        let (g, eids) = graph_with_limits(&[80, 50]);
        let mut r = route_on(&eids);
        r.start_virtual = Some((eids[0], 40.0, true)); // 已行驶 40m
        let breaks = speed_breaks_ahead(&r, &g, 1000.0);
        // 首边剩余 60m → 断点在 60m 处
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
