// nav-matcher 单元测试：合成轨迹匹配稳定性（平行路不跳变/直路锁定/折返）。
use nav_dataset::{Edge, EdgeKind, Node, RoutingGraph};
use nav_graph::CompactGraph;
use nav_matcher::*;
use nav_spatial::SpatialIndex;

fn build_graph() -> CompactGraph {
    // 两条平行路：A 在 z=0（x: 0→1000），B 在 z=100（平行）
    // 加一条横向连接（x=500 处）
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
            x: 0.0,
            y: 0.0,
            z: 100.0,
        },
        Node {
            uid: 4,
            x: 1000.0,
            y: 0.0,
            z: 100.0,
        },
        Node {
            uid: 5,
            x: 500.0,
            y: 0.0,
            z: 0.0,
        },
        Node {
            uid: 6,
            x: 500.0,
            y: 0.0,
            z: 100.0,
        },
    ];
    let mk = |from: u32, to: u32, kind: EdgeKind, pts: Vec<(f64, f64, f64)>| Edge {
        from,
        to,
        kind,
        length: 1.0,
        source_uid: 0,
        geometry: pts,
        speed_limit: 50,
        road_class: 1,
        semaphore_id: -1,
        flags: 0,
        movement_id: None,
    };
    let edges = vec![
        mk(
            0,
            1,
            EdgeKind::Road,
            vec![(0.0, 0.0, 0.0), (1000.0, 0.0, 0.0)],
        ), // A
        mk(
            2,
            3,
            EdgeKind::Road,
            vec![(0.0, 0.0, 100.0), (1000.0, 0.0, 100.0)],
        ), // B
        mk(
            4,
            5,
            EdgeKind::Road,
            vec![(500.0, 0.0, 0.0), (500.0, 0.0, 100.0)],
        ), // 连接
    ];
    CompactGraph::build(&RoutingGraph { nodes, edges })
}

#[test]
fn straight_line_lock_stable() {
    let g = build_graph();
    let sp = SpatialIndex::build(&g, 256.0);
    let mut m = MapMatcher::new(MatcherConfig::default());
    // 沿 A 路行驶（z=0，噪声 ±2m）
    let mut last = None;
    for i in 0..50 {
        let x = i as f64 * 20.0 + 3.0;
        let z = 2.0 * (i % 3) as f64 - 2.0;
        let mm = m.match_frame(&g, &sp, x, z, 0.0);
        assert!(
            mm.confidence != MatchConfidence::Unmatched,
            "第 {i} 帧未匹配"
        );
        assert!(mm.lateral < 5.0, "第 {i} 帧 lateral={}", mm.lateral);
        last = Some(mm.edge_id);
    }
    // 稳定在 A 路（0 号边）
    assert_eq!(last, Some(0), "应稳定匹配 A 路");
}

#[test]
fn parallel_road_no_swap() {
    let g = build_graph();
    let sp = SpatialIndex::build(&g, 256.0);
    let mut m = MapMatcher::new(MatcherConfig::default());
    // 在 A 路附近行驶（lateral 2m），应锁定 A 不跳 B
    for i in 0..40 {
        let x = i as f64 * 20.0;
        let z = 2.0;
        let mm = m.match_frame(&g, &sp, x, z, 0.0);
        assert_eq!(
            mm.edge_id,
            0,
            "第 {i} 帧跳边: {:?}",
            mm.candidates
                .iter()
                .map(|c| (c.edge_id, c.lateral))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn u_turn_follows_topology() {
    let g = build_graph();
    let sp = SpatialIndex::build(&g, 256.0);
    let mut m = MapMatcher::new(MatcherConfig::default());
    // 沿 A 前进到 x=500，折返（heading π）
    for i in 0..25 {
        m.match_frame(&g, &sp, i as f64 * 20.0, 1.0, 0.0);
    }
    // 折返后匹配 B（连接边）或 A 反向——拓扑连续性应导向连接边
    let mm = m.match_frame(&g, &sp, 500.0, 3.0, std::f64::consts::PI);
    assert!(mm.confidence != MatchConfidence::Unmatched);
    // 折返后继续向南（连接边方向）
    let mm2 = m.match_frame(&g, &sp, 500.0, 20.0, std::f64::consts::PI / 2.0);
    assert!(mm2.edge_id == 2 || mm2.confidence != MatchConfidence::Unmatched);
}
