// P2-09 搜索测试：A* 最优性（vs Dijkstra Oracle §71）、虚拟起终点（§57）、已知路径正确性。
use nav_dataset::{Edge, EdgeKind, Node, RoutingGraph};
use nav_graph::CompactGraph;
use nav_router::cost::RouteProfile;
use nav_router::search::{route_cost, RouteRequest, Router};
use nav_router::snap::{SnapPoint, VirtualEndpoint};

/// 网格图：3x3 网格 + 对角捷径（已知最短路径）。
fn grid_graph() -> CompactGraph {
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
    let mk = |from: u32, to: u32, speed: i16| Edge {
        from,
        to,
        kind: EdgeKind::Road,
        length: 1.0,
        source_uid: 0,
        geometry: vec![
            (nodes[from as usize].x, 0.0, nodes[from as usize].z),
            (nodes[to as usize].x, 0.0, nodes[to as usize].z),
        ],
        speed_limit: speed,
        road_class: 1,
        semaphore_id: -1,
        flags: 0,
        movement_id: None,
    };
    // 水平边
    for i in 0..3 {
        for j in 0..2 {
            edges.push(mk(n(i, j), n(i, j + 1), 60));
            edges.push(mk(n(i, j + 1), n(i, j), 60));
        }
    }
    // 垂直边
    for i in 0..2 {
        for j in 0..3 {
            edges.push(mk(n(i, j), n(i + 1, j), 60));
            edges.push(mk(n(i + 1, j), n(i, j), 60));
        }
    }
    // 对角捷径（0,0 → 2,2）
    edges.push(mk(n(0, 0), n(2, 2), 60));
    edges.push(mk(n(2, 2), n(0, 0), 60));
    CompactGraph::build(&RoutingGraph { nodes, edges })
}

fn node_snap(g: &CompactGraph, idx: u32) -> SnapPoint {
    let p = g.positions[idx as usize];
    // 用 edge 起点构造（offset 0 的虚拟点：edge 从 idx 出发的第一条出边）
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
fn astar_matches_dijkstra_grid() {
    let g = grid_graph();
    let mut r = Router::new(g.node_count());
    for profile in [
        RouteProfile::Shortest,
        RouteProfile::Fastest,
        RouteProfile::Balanced,
    ] {
        for (a, b) in [(0u32, 8u32), (0, 4), (4, 8), (8, 0), (1, 7)] {
            let start = VirtualEndpoint::start(&node_snap(&g, a), false);
            let goal = VirtualEndpoint::goal(&node_snap(&g, b));
            let req = RouteRequest::new(&g, start, goal, profile);
            let da = r.dijkstra(&req).expect("dijkstra 应找到路线");
            let aa = r.astar(&req).expect("astar 应找到路线");
            let cd = route_cost(&g, &da, profile);
            let ca = route_cost(&g, &aa, profile);
            assert!(
                (cd - ca).abs() < 1e-6,
                "profile={profile:?} {a}→{b}: Dijkstra={cd:.6} A*={ca:.6} 不一致"
            );
        }
    }
}

#[test]
fn known_shortest_path() {
    let g = grid_graph();
    let mut r = Router::new(g.node_count());
    // (0,0) → (2,2)：对角捷径 2828m（√(2000²+2000²)）< 网格 4000m
    let start = VirtualEndpoint::start(&node_snap(&g, 0), false);
    let goal = VirtualEndpoint::goal(&node_snap(&g, 8));
    let req = RouteRequest::new(&g, start, goal, RouteProfile::Shortest);
    let route = r.astar(&req).unwrap();
    assert!(
        (route.distance_m - 2828.427).abs() < 1.0,
        "distance={}",
        route.distance_m
    );
    assert_eq!(route.edges.len(), 1, "应走对角捷径");
}

#[test]
fn virtual_mid_edge_start() {
    let g = grid_graph();
    let mut r = Router::new(g.node_count());
    // 起点在 (0,0)→(1,0) 边中部（offset 500）
    let eid = g.out_edges(0)[0];
    let snap = SnapPoint {
        edge_id: eid,
        offset: 500.0,
        position: (0.0, 0.0, 500.0),
        tangent: 0.0,
        lateral: 0.0,
    };
    let start = VirtualEndpoint::start(&snap, false);
    // 终点 (0,2)
    let goal = VirtualEndpoint::goal(&node_snap(&g, 2));
    let req = RouteRequest::new(&g, start, goal, RouteProfile::Shortest);
    let route = r.astar(&req).unwrap();
    assert!(route.distance_m > 0.0);
    // 起点虚拟段应为 forward（500→1000 剩余段）
    let (se, soff, fwd) = route.start_virtual.unwrap();
    assert_eq!(se, eid);
    assert!((soff - 500.0).abs() < 1e-9);
    assert!(fwd);
    // 总距离 = 500（剩余段）+ 1000（垂直边）= 1500
    assert!(
        (route.distance_m - 1500.0).abs() < 1.0,
        "distance={}",
        route.distance_m
    );
}

#[test]
fn single_edge_route_direct() {
    let g = grid_graph();
    let mut r = Router::new(g.node_count());
    let eid = g.out_edges(0)[0];
    let s = SnapPoint {
        edge_id: eid,
        offset: 100.0,
        position: (0.0, 0.0, 100.0),
        tangent: 0.0,
        lateral: 0.0,
    };
    let go = SnapPoint {
        edge_id: eid,
        offset: 800.0,
        position: (0.0, 0.0, 800.0),
        tangent: 0.0,
        lateral: 0.0,
    };
    let req = RouteRequest::new(
        &g,
        VirtualEndpoint::start(&s, false),
        VirtualEndpoint::goal(&go),
        RouteProfile::Shortest,
    );
    let route = r.astar(&req).unwrap();
    assert!(
        (route.distance_m - 700.0).abs() < 1e-6,
        "distance={}",
        route.distance_m
    );
    assert!(route.edges.is_empty(), "单段路线不应有图边");
}
