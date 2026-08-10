// P2-10 Alternatives（§79-82）：
// 三 profile 各搜一次 → overlap 去重（§80）→ 不足 2 条时最优路线边加 overlap penalty 重搜（§81）→ 质量检查（§82）。
use nav_graph::CompactGraph;

use crate::cost::RouteProfile;
use crate::search::{route_cost, Route, RouteRequest, Router};
use crate::snap::VirtualEndpoint;

/// 备选路线规划参数。
pub struct AltParams {
    /// 视为重复的 overlap 阈值（§80：共享长度 / min(LA,LB)）。
    pub overlap_threshold: f64,
    /// 加 penalty 重搜时每边额外成本系数（× 边原始成本）。
    pub penalty_factor: f64,
    /// 质量：候选成本超过最优该倍数视为劣化拒绝（§82）。
    pub max_cost_ratio: f64,
}

impl Default for AltParams {
    fn default() -> Self {
        AltParams {
            overlap_threshold: 0.85,
            penalty_factor: 2.0,
            max_cost_ratio: 2.0,
        }
    }
}

/// 备选路线集合（按成本升序；1～3 条，§82 无合理备选只返回一条）。
pub struct Alternatives {
    pub routes: Vec<Route>,
    /// 各路线使用的 profile（与 routes 一一对应）。
    pub profiles: Vec<RouteProfile>,
}

/// 规划备选路线。
pub fn plan_alternatives(
    graph: &CompactGraph,
    router: &mut Router,
    start: VirtualEndpoint,
    goal: VirtualEndpoint,
    params: &AltParams,
) -> Alternatives {
    // 1) 三 profile 各搜一次（§79）
    let mut routes: Vec<(Route, RouteProfile)> = Vec::new();
    for profile in [
        RouteProfile::Fastest,
        RouteProfile::Shortest,
        RouteProfile::Balanced,
    ] {
        let req = RouteRequest::new(graph, start, goal, profile);
        if let Some(r) = router.astar(&req) {
            if !already_covered(graph, &routes, &r, params.overlap_threshold) {
                routes.push((r, profile));
            }
        }
    }
    // 2) 独立路线不足 2 条 → 最优路线边加 penalty 重搜（§81）
    if routes.len() < 2 {
        if let Some((best, profile)) = routes.first().cloned() {
            let mut penalties = vec![0.0f64; graph.edges.len()];
            let base_cost = route_cost(graph, &best, profile);
            let per_edge = base_cost / best.edges.len().max(1) as f64 * params.penalty_factor;
            for &eid in &best.edges {
                penalties[eid as usize] = per_edge;
            }
            let req = RouteRequest::with_penalties(graph, start, goal, profile, penalties);
            if let Some(r) = router.astar(&req) {
                if !already_covered(graph, &routes, &r, params.overlap_threshold) {
                    routes.push((r, profile));
                }
            }
        }
    }
    // 3) 质量检查（§82）：成本劣化拒绝；排序
    if let Some((best, _)) = routes.first() {
        let best_cost = route_cost(graph, best, routes[0].1);
        routes.retain(|(r, p)| route_cost(graph, r, *p) <= best_cost * params.max_cost_ratio);
    }
    routes.sort_by(|a, b| {
        route_cost(graph, &a.0, a.1)
            .partial_cmp(&route_cost(graph, &b.0, b.1))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    routes.truncate(3);
    let profiles = routes.iter().map(|(_, p)| *p).collect();
    let routes = routes.into_iter().map(|(r, _)| r).collect();
    Alternatives { routes, profiles }
}

/// 与已有路线集合的 overlap 是否超阈值（§80）。
fn already_covered(
    graph: &CompactGraph,
    existing: &[(Route, RouteProfile)],
    cand: &Route,
    threshold: f64,
) -> bool {
    for (r, p) in existing {
        let overlap = overlap_ratio(graph, r, cand, *p);
        if overlap >= threshold {
            return true;
        }
    }
    false
}

/// overlap = 共享长度 / min(LA, LB)（§80 定义）。
pub fn overlap_ratio(graph: &CompactGraph, a: &Route, b: &Route, _profile: RouteProfile) -> f64 {
    let edges_a: std::collections::HashSet<u32> = a.edges.iter().copied().collect();
    let edges_b: std::collections::HashSet<u32> = b.edges.iter().copied().collect();
    let shared: f64 = edges_a
        .intersection(&edges_b)
        .map(|&e| edge_len(graph, e))
        .sum();
    let la: f64 = a.edges.iter().map(|&e| edge_len(graph, e)).sum();
    let lb: f64 = b.edges.iter().map(|&e| edge_len(graph, e)).sum();
    if la < 1e-9 && lb < 1e-9 {
        return 1.0;
    }
    shared / la.min(lb).max(1e-9)
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
    use crate::snap::SnapPoint;
    use nav_dataset::{Edge, EdgeKind, Node, RoutingGraph};

    /// 井字形图：0↔8 有多条路径（上下两绕行）。
    fn h_graph() -> CompactGraph {
        let mut nodes = Vec::new();
        let mut uid = 1u64;
        for i in 0..3 {
            for j in 0..3 {
                nodes.push(Node {
                    uid,
                    x: j as f64 * 1000.0,
                    y: 0.0,
                    z: i as f64 * 1000.0,
                });
                uid += 1;
            }
        }
        let n = |i: usize, j: usize| -> u32 { (i * 3 + j) as u32 };
        let mut edges = Vec::new();
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
        for i in 0..3 {
            for j in 0..2 {
                edges.push(mk(n(i, j), n(i, j + 1)));
                edges.push(mk(n(i, j + 1), n(i, j)));
            }
        }
        for i in 0..2 {
            for j in 0..3 {
                edges.push(mk(n(i, j), n(i + 1, j)));
                edges.push(mk(n(i + 1, j), n(i, j)));
            }
        }
        CompactGraph::build(&RoutingGraph { nodes, edges })
    }

    fn node_snap(g: &CompactGraph, idx: u32) -> SnapPoint {
        let p = g.positions[idx as usize];
        let eid = g.out_edges(idx as usize)[0];
        SnapPoint {
            edge_id: eid,
            offset: 0.0,
            position: p,
            tangent: 0.0,
            lateral: 0.0,
        }
    }

    #[test]
    fn alternatives_find_distinct_routes() {
        let g = h_graph();
        let mut r = Router::new(g.node_count());
        let start = VirtualEndpoint::start(&node_snap(&g, 0), false);
        let goal = VirtualEndpoint::goal(&node_snap(&g, 8));
        let alts = plan_alternatives(&g, &mut r, start, goal, &AltParams::default());
        assert!(
            alts.routes.len() >= 2,
            "应至少 2 条独立路线: {}",
            alts.routes.len()
        );
        for i in 0..alts.routes.len() {
            for j in (i + 1)..alts.routes.len() {
                let o = overlap_ratio(&g, &alts.routes[i], &alts.routes[j], alts.profiles[i]);
                assert!(o < 0.85, "路线 {i}/{j} overlap {o:.2} 过高");
            }
        }
    }

    #[test]
    fn alternatives_sorted_by_cost() {
        let g = h_graph();
        let mut r = Router::new(g.node_count());
        let start = VirtualEndpoint::start(&node_snap(&g, 0), false);
        let goal = VirtualEndpoint::goal(&node_snap(&g, 8));
        let alts = plan_alternatives(&g, &mut r, start, goal, &AltParams::default());
        for w in alts.routes.windows(2) {
            let c1 = route_cost(&g, &w[0], RouteProfile::Shortest);
            let c2 = route_cost(&g, &w[1], RouteProfile::Shortest);
            assert!(c1 <= c2 + 1e-6, "成本应升序: {c1} > {c2}");
        }
    }
}
