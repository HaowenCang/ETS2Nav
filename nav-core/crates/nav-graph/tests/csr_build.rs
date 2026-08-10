// nav-graph CSR 构建单元测试：手工 RoutingGraph → 压缩 + 边连续性 + 反向邻接。
use nav_dataset::{Edge, EdgeKind, Node, RoutingGraph};

fn sample_graph() -> RoutingGraph {
    let nodes = (1..=5u64)
        .map(|uid| Node {
            uid,
            x: uid as f64,
            y: 0.0,
            z: 0.0,
        })
        .collect();
    let mk = |from: u32, to: u32, kind: EdgeKind| Edge {
        from,
        to,
        kind,
        length: 1.0,
        source_uid: 0,
        geometry: vec![(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)],
        speed_limit: 50,
        road_class: 1,
        semaphore_id: -1,
        flags: 0,
        movement_id: None,
    };
    let edges = vec![
        mk(0, 1, EdgeKind::Road),
        mk(1, 2, EdgeKind::Road),
        mk(2, 0, EdgeKind::JunctionMovement),
        mk(3, 4, EdgeKind::Ferry),
    ];
    RoutingGraph { nodes, edges }
}

#[test]
fn csr_compaction_and_continuity() {
    let rg = sample_graph();
    let c = nav_graph::CompactGraph::build(&rg);
    assert_eq!(c.node_count(), 5);
    for e in &c.edges {
        let outs = c.out_edges(e.from as usize);
        assert!(
            outs.iter().any(|&id| {
                let t = &c.edges[id as usize];
                t.from == e.from && t.to == e.to
            }),
            "from={} to={} 的出边缺失",
            e.from,
            e.to
        );
    }
    for e in &c.edges {
        let ins = c.in_edges(e.to as usize);
        assert!(ins.iter().any(|&id| c.edges[id as usize].to == e.to));
    }
    assert_eq!(c.node_index(3), Some(2));
}

#[test]
fn csr_drops_inactive_nodes() {
    let mut rg = sample_graph();
    rg.nodes.push(Node {
        uid: 99,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    });
    let c = nav_graph::CompactGraph::build(&rg);
    assert_eq!(c.node_count(), 5);
    assert!(c.node_index(99).is_none());
}
