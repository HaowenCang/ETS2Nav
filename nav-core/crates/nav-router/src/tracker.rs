// P2-11 Route Tracker（§83-87）：
// 保存 route 进度（edge index/offset/travelled/remaining/next maneuver index）；
// Route Matching 用局部窗口（§84）；suffix 预计算 O(1) 剩余（§85/86）；单调性防跳变（§87）。
use nav_graph::CompactGraph;

use crate::search::Route;

/// 每帧进度更新结果。
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    /// 当前 route 边索引（窗口内匹配成功时有效）。
    pub edge_index: usize,
    /// 沿 route 方向的边内 offset（米）。
    pub edge_offset: f64,
    pub distance_travelled: f64,
    pub remaining_distance: f64,
    pub remaining_time: f64,
    /// 本帧是否在 route 局部窗口内匹配到（false = 离路线证据，P2-12）。
    pub matched: bool,
    /// 本帧前进距离（米；倒车/平行跳动为 0，单调性 §87）。
    pub progressed: f64,
}

/// 路线进度跟踪器。
pub struct RouteTracker {
    route: Route,
    /// suffix_distance[i] = edges[i..] 总长（含虚拟段；§85）。
    suffix_distance: Vec<f64>,
    /// suffix_time[i] = edges[i..] 总时间（§86）。
    suffix_time: Vec<f64>,
    edge_index: usize,
    edge_offset: f64,
    distance_travelled: f64,
    /// 匹配窗口（route 边索引 ± window）。
    window: usize,
}

impl RouteTracker {
    /// 构造：预计算 suffix（O(E) 一次，之后 O(1)）。
    pub fn new(graph: &CompactGraph, route: Route, window: usize) -> Self {
        let n = route.edges.len();
        let mut sd = vec![0.0f64; n + 1];
        let mut st = vec![0.0f64; n + 1];
        for i in (0..n).rev() {
            sd[i] = sd[i + 1] + edge_len(graph, route.edges[i]);
            st[i] = st[i + 1] + time_of(graph, route.edges[i]);
        }
        RouteTracker {
            route,
            suffix_distance: sd,
            suffix_time: st,
            edge_index: 0,
            edge_offset: 0.0,
            distance_travelled: 0.0,
            window,
        }
    }

    /// 每帧更新：匹配 edge 在窗口内 → 推进进度；否则标记未匹配。
    pub fn update(
        &mut self,
        graph: &CompactGraph,
        matched_edge: u32,
        matched_offset: f64,
    ) -> ProgressUpdate {
        // 纯虚拟段路线（起点/终点都在同一或相邻边中部——无中间图边）：
        // 剩余 = 全程距离；匹配到终点虚拟段所在边 → 到达（剩余 0）。
        if self.route.edges.is_empty() {
            let end_edge = self.route.end_virtual.map(|(e, _, _)| e);
            let remaining = if end_edge == Some(matched_edge) {
                0.0
            } else {
                self.route.distance_m
            };
            return ProgressUpdate {
                edge_index: 0,
                edge_offset: 0.0,
                distance_travelled: 0.0,
                remaining_distance: remaining,
                remaining_time: remaining / 14.0,
                matched: true,
                progressed: 0.0,
            };
        }
        // §84：局部窗口搜索
        let lo = self.edge_index.saturating_sub(self.window);
        let hi = (self.edge_index + self.window).min(self.route.edges.len().saturating_sub(1));
        let mut found: Option<usize> = None;
        // 反向行驶标记：matched 边是窗口内边的**双向对边**（同一几何）——识别但不推进（§87）
        let mut reversed = false;
        let matched_ok = (matched_edge as usize) < graph.edges.len();
        let matched_e = if matched_ok {
            Some(&graph.edges[matched_edge as usize])
        } else {
            None
        };
        for i in lo..=hi {
            if self.route.edges[i] == matched_edge {
                found = Some(i);
                break;
            }
        }
        if found.is_none() {
            if let Some(me) = matched_e {
                for i in lo..=hi {
                    let re = &graph.edges[self.route.edges[i] as usize];
                    // 同一几何（geom_start/len 相同）= 同一道路的双向边；反向行驶
                    if re.geom_start == me.geom_start
                        && re.geom_len == me.geom_len
                        && re.kind == me.kind
                    {
                        found = Some(i);
                        reversed = true;
                        break;
                    }
                }
            }
        }
        let mut progressed = 0.0;
        if let Some(idx) = found {
            if reversed {
                // 对向边：匹配但进度不推进（防平行/双向路抖动——§87）
                return ProgressUpdate {
                    edge_index: self.edge_index,
                    edge_offset: self.edge_offset,
                    distance_travelled: self.distance_travelled,
                    remaining_distance: self.edge_remaining(graph),
                    remaining_time: self.time_remaining(graph),
                    matched: true,
                    progressed: 0.0,
                };
            }
            let prev = self.edge_index;
            if idx > prev {
                // 前进：当前边剩余段 + 新边 offset 计入 travelled（单调 §87；避免重复计数）
                let cur = self.route.edges[prev];
                let cur_len = edge_len(graph, cur);
                self.distance_travelled += (cur_len - self.edge_offset).max(0.0) + matched_offset;
                // 中间经过的完整边
                for e in (prev + 1)..idx {
                    self.distance_travelled += edge_len(graph, self.route.edges[e]);
                }
                self.edge_index = idx;
                self.edge_offset = matched_offset;
                progressed = matched_offset;
            } else if idx == prev {
                // 同边：offset 前进则计入（倒车/停车不减少）
                if matched_offset > self.edge_offset {
                    progressed = matched_offset - self.edge_offset;
                    self.distance_travelled += progressed;
                    self.edge_offset = matched_offset;
                }
            }
            // idx < prev：回退到已走过的边（U-turn/重走）——不减少 travelled
        }
        ProgressUpdate {
            edge_index: self.edge_index,
            edge_offset: self.edge_offset,
            distance_travelled: self.distance_travelled,
            remaining_distance: self.edge_remaining(graph),
            remaining_time: self.time_remaining(graph),
            matched: found.is_some(),
            progressed,
        }
    }

    /// 剩余距离（O(1)，§85）：当前边剩余 + suffix。
    pub fn edge_remaining(&self, graph: &CompactGraph) -> f64 {
        if self.route.edges.is_empty() {
            return self.route.distance_m;
        }
        let eid = self.route.edges[self.edge_index.min(self.route.edges.len() - 1)];
        let len = edge_len(graph, eid);
        let rem = (len - self.edge_offset).max(0.0);
        rem + self.suffix_distance[self.edge_index + 1]
    }

    /// 剩余时间（O(1)，§86）。
    pub fn time_remaining(&self, graph: &CompactGraph) -> f64 {
        if self.route.edges.is_empty() {
            return 0.0;
        }
        let eid = self.route.edges[self.edge_index.min(self.route.edges.len() - 1)];
        let seg = (edge_len(graph, eid) - self.edge_offset).max(0.0) / speed_of(graph, eid);
        seg + self.suffix_time[self.edge_index + 1]
    }

    /// 总进度（0..1）。
    pub fn progress(&self) -> f64 {
        let total = self.suffix_distance[0];
        if total <= 1e-9 {
            return 1.0;
        }
        (self.distance_travelled / total).clamp(0.0, 1.0)
    }

    /// 当前 route 边索引（P2-13 maneuver 用）。
    pub fn edge_index(&self) -> usize {
        self.edge_index
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

fn time_of(g: &CompactGraph, eid: u32) -> f64 {
    edge_len(g, eid) / speed_of(g, eid)
}

fn speed_of(g: &CompactGraph, eid: u32) -> f64 {
    let e = &g.edges[eid as usize];
    let kph = match e.kind {
        nav_graph::EdgeKind::Road => match e.speed_limit {
            s if s < 0 => 50.0,
            0 => 130.0,
            s => s as f64,
        },
        nav_graph::EdgeKind::JunctionMovement => 30.0,
        nav_graph::EdgeKind::Ferry | nav_graph::EdgeKind::Train => 30.0,
        nav_graph::EdgeKind::ServiceAccess => 50.0,
    };
    kph / 3.6
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::RouteProfile;
    use crate::search::Route;
    use nav_dataset::{Edge, EdgeKind, Node, RoutingGraph};

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
                x: 2000.0,
                y: 0.0,
                z: 0.0,
            },
            Node {
                uid: 4,
                x: 3000.0,
                y: 0.0,
                z: 0.0,
            },
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
        let edges = vec![mk(0, 1), mk(1, 2), mk(2, 3)];
        CompactGraph::build(&RoutingGraph { nodes, edges })
    }

    fn route_of(g: &CompactGraph) -> Route {
        let edges: Vec<u32> = (0..3).collect();
        let dist: f64 = edges.iter().map(|&e| edge_len(g, e)).sum();
        Route {
            profile: RouteProfile::Fastest,
            edges,
            start_virtual: None,
            end_virtual: None,
            distance_m: dist,
            eta_s: 60.0,
            road_edge_count: 3,
            junction_count: 0,
            signal_count: 0,
            ferry_count: 0,
            train_count: 0,
            gps_avoid_distance: 0.0,
            unknown_speed_distance: 0.0,
        }
    }

    #[test]
    fn progress_monotonic_along_route() {
        let g = graph();
        let mut t = RouteTracker::new(&g, route_of(&g), 2);
        let mut last = 0.0;
        for (e, off) in [
            (0u32, 100.0),
            (0, 500.0),
            (0, 900.0),
            (1, 100.0),
            (1, 900.0),
            (2, 100.0),
            (2, 500.0),
        ] {
            let u = t.update(&g, e, off);
            assert!(u.matched, "应匹配到 route 边");
            assert!(
                u.distance_travelled >= last,
                "单调性破坏: {} < {}",
                u.distance_travelled,
                last
            );
            last = u.distance_travelled;
        }
        assert!(
            (t.edge_remaining(&g) - 500.0).abs() < 1e-6,
            "remaining={}",
            t.edge_remaining(&g)
        );
        assert!(
            (t.progress() - 2500.0 / 3000.0).abs() < 1e-6,
            "progress={}",
            t.progress()
        );
    }

    #[test]
    fn reverse_drive_does_not_decrease() {
        let g = graph();
        let mut t = RouteTracker::new(&g, route_of(&g), 2);
        t.update(&g, 0, 800.0);
        let u = t.update(&g, 0, 200.0);
        assert!(
            (u.distance_travelled - 800.0).abs() < 1e-9,
            "倒车不应减少: {}",
            u.distance_travelled
        );
        assert!((u.progressed - 0.0).abs() < 1e-9);
    }

    #[test]
    fn parallel_edge_hop_stays_stable() {
        let g = graph();
        let mut t = RouteTracker::new(&g, route_of(&g), 2);
        t.update(&g, 0, 100.0);
        let u = t.update(&g, 999, 0.0); // 窗口外（平行跳动/偏航）
        assert!(!u.matched, "窗口外应 unmatched");
        let u2 = t.update(&g, 0, 150.0);
        assert!(u2.matched);
        assert!((u2.distance_travelled - 150.0).abs() < 1e-9);
    }
}
