// P2-07 Snap Model（§54-57）：
// SnapPoint 定义；起点用当前 Match（不重新 nearest-node）；目的地用最近可路由 edge；
// Virtual Start/Goal 不修改全局图，route search 从 edge 中部出发。
use nav_graph::{CompactGraph, EdgeKind};
use nav_spatial::SpatialIndex;

/// 吸附点：edge + 弧长 offset + 世界位置（§54）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapPoint {
    pub edge_id: u32,
    pub offset: f64,
    pub position: (f64, f64, f64),
    /// 投影点 tangent（世界弧度）。
    pub tangent: f64,
    /// 横向距离（米）。
    pub lateral: f64,
}

impl SnapPoint {
    /// 从 MapMatch 直接构造起点 snap（§55：使用当前 edge+offset，不再次 nearest-node）。
    pub fn from_match(
        graph: &CompactGraph,
        edge_id: u32,
        offset: f64,
        position: (f64, f64, f64),
    ) -> Self {
        let e = &graph.edges[edge_id as usize];
        let pts = graph.edge_geometry(e);
        let tangent = if pts.len() >= 2 {
            // 用 offset 所在段的 tangent（投影点方向）
            let (_, _, _, t) = nav_graph::project_point(pts, position.0, position.2);
            t
        } else {
            let a = graph.positions[e.from as usize];
            let b = graph.positions[e.to as usize];
            (b.2 - a.2).atan2(b.0 - a.0)
        };
        SnapPoint {
            edge_id,
            offset,
            position,
            tangent,
            lateral: 0.0,
        }
    }

    /// 该 edge 的总长（用于剩余 forward/backward 段计算）。
    pub fn edge_length(&self, graph: &CompactGraph) -> f64 {
        let pts = graph.edge_geometry(&graph.edges[self.edge_id as usize]);
        if pts.len() >= 2 {
            nav_graph::polyline_length(pts)
        } else {
            let a = graph.positions[graph.edges[self.edge_id as usize].from as usize];
            let b = graph.positions[graph.edges[self.edge_id as usize].to as usize];
            ((a.0 - b.0).powi(2) + (a.2 - b.2).powi(2)).sqrt()
        }
    }
}

/// 虚拟起点/终点（§57）：edge + offset + 允许方向。
/// Route search 使用这些段构建虚拟搜索：不修改全局图。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualEndpoint {
    pub edge_id: u32,
    /// 沿 edge 的 offset（米）。
    pub offset: f64,
    /// 是否允许向 edge 前进方向（forward）行驶。
    pub allow_forward: bool,
    /// 是否允许向 edge 反向（backward）行驶（起点回头/终点反向到达）。
    pub allow_backward: bool,
}

impl VirtualEndpoint {
    /// 起点：从当前 snap 出发（§57 支持 forward + 可选 backward）。
    pub fn start(snap: &SnapPoint, allow_backward: bool) -> Self {
        VirtualEndpoint {
            edge_id: snap.edge_id,
            offset: snap.offset,
            allow_forward: true,
            allow_backward,
        }
    }

    /// 终点：到达目标 snap（§57 支持 forward 到达，即沿 edge 方向到目标点）。
    pub fn goal(snap: &SnapPoint) -> Self {
        VirtualEndpoint {
            edge_id: snap.edge_id,
            offset: snap.offset,
            allow_forward: true,
            allow_backward: false,
        }
    }
}

/// 目的地吸附（§56）：任意坐标 → 最近可路由 edge（spatial 查询 + 逐边投影）。
/// 返回半径内最近的可路由边（Road/Movement；Ferry/Train 不吸附——§56 可路由 edge）。
pub fn snap_nearest(
    graph: &CompactGraph,
    spatial: &SpatialIndex,
    x: f64,
    z: f64,
    radius: f64,
) -> Option<SnapPoint> {
    let mut best: Option<SnapPoint> = None;
    for eid in spatial.query_radius(x, z, radius) {
        let e = &graph.edges[eid as usize];
        if e.kind != EdgeKind::Road && e.kind != EdgeKind::JunctionMovement {
            continue;
        }
        let pts = graph.edge_geometry(e);
        if pts.len() < 2 {
            continue;
        }
        let (proj, lateral, offset, tangent) = nav_graph::project_point(pts, x, z);
        if best
            .as_ref()
            .is_none_or(|b: &SnapPoint| lateral < b.lateral)
        {
            best = Some(SnapPoint {
                edge_id: eid,
                offset,
                position: proj,
                tangent,
                lateral,
            });
        }
    }
    best
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
                x: 0.0,
                y: 0.0,
                z: 1000.0,
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
            ),
            mk(
                0,
                2,
                EdgeKind::Road,
                vec![(0.0, 0.0, 0.0), (0.0, 0.0, 1000.0)],
            ),
        ];
        RoutingGraph { nodes, edges }
    }

    #[test]
    fn snap_nearest_picks_closest_edge() {
        let g = CompactGraph::build(&rg());
        let sp = SpatialIndex::build(&g, 256.0);
        // 点在 (600, 3) —— 距横边 3m，距纵边 597m → 吸附横边
        let s = snap_nearest(&g, &sp, 600.0, 3.0, 200.0).expect("应找到");
        assert_eq!(s.edge_id, 0);
        assert!((s.offset - 600.0).abs() < 0.5, "offset={}", s.offset);
        assert!(s.lateral < 3.1, "lateral={}", s.lateral);
    }

    #[test]
    fn snap_nearest_out_of_radius_none() {
        let g = CompactGraph::build(&rg());
        let sp = SpatialIndex::build(&g, 256.0);
        assert!(snap_nearest(&g, &sp, 5000.0, 5000.0, 100.0).is_none());
    }

    #[test]
    fn virtual_endpoint_segments() {
        let g = CompactGraph::build(&rg());
        let sp = SpatialIndex::build(&g, 256.0);
        let s = snap_nearest(&g, &sp, 400.0, 0.0, 200.0).unwrap();
        let st = VirtualEndpoint::start(&s, true);
        assert!(st.allow_forward && st.allow_backward);
        // forward 剩余 = L - offset = 1000 - 400 = 600
        assert!((s.edge_length(&g) - s.offset - 600.0).abs() < 1e-6);
        let g2 = VirtualEndpoint::goal(&s);
        assert!(g2.allow_forward && !g2.allow_backward);
    }
}
