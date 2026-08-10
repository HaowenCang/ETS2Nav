// P2-09 Dijkstra Oracle + A*（§70-78）。
// Dijkstra 作 reference（验证 A* 最优性）；A* 正式搜索（binary heap + generation counter 预分配）。
// 虚拟起终点（§57）：不改全局图，起点可 forward/backward 展开，终点经虚拟段到达。
use nav_graph::CompactGraph;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

/// f64 有序包装（BinaryHeap 需要 Ord；f64 仅 PartialOrd）。
#[derive(Clone, Copy)]
struct F(f64);
impl PartialEq for F {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for F {}
impl PartialOrd for F {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for F {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

use crate::cost::{EdgeCostProvider, RouteProfile};
use crate::snap::VirtualEndpoint;

/// 路线结果（§76/77）。
#[derive(Debug, Clone)]
pub struct Route {
    pub profile: RouteProfile,
    /// 图边序列（有序）。
    pub edges: Vec<u32>,
    /// 起点虚拟段（edge_id, offset, forward=true 表示沿前进方向）。
    pub start_virtual: Option<(u32, f64, bool)>,
    /// 终点虚拟段（edge_id, offset, forward）。
    pub end_virtual: Option<(u32, f64, bool)>,
    /// 总距离（米）。
    pub distance_m: f64,
    /// 静态自由流 ETA（秒，§78）。
    pub eta_s: f64,
    pub road_edge_count: u32,
    pub junction_count: u32,
    pub signal_count: u32,
    pub ferry_count: u32,
    pub train_count: u32,
    pub gps_avoid_distance: f64,
    pub unknown_speed_distance: f64,
}

/// 搜索请求（构造时绑定 profile 成本）。
pub struct RouteRequest<'a> {
    pub graph: &'a CompactGraph,
    pub start: VirtualEndpoint,
    pub goal: VirtualEndpoint,
    pub profile: RouteProfile,
    cost: EdgeCostProvider,
    /// 每边额外成本（P2-10 overlap penalty 注入；空 = 无惩罚）。
    pub edge_penalties: Vec<f64>,
}

impl<'a> RouteRequest<'a> {
    pub fn new(
        graph: &'a CompactGraph,
        start: VirtualEndpoint,
        goal: VirtualEndpoint,
        profile: RouteProfile,
    ) -> Self {
        RouteRequest {
            graph,
            start,
            goal,
            profile,
            cost: EdgeCostProvider::new(profile),
            edge_penalties: Vec::new(),
        }
    }

    /// 边主成本（profile 决定）+ 注入 penalty（P2-10）。
    fn edge_cost(&self, eid: u32) -> f64 {
        let base = self.cost.edge_cost(self.graph, eid);
        if eid as usize >= self.edge_penalties.len() {
            base
        } else {
            base + self.edge_penalties[eid as usize]
        }
    }

    /// 带边惩罚的构造（P2-10 overlap penalty 重搜，§81）。
    pub fn with_penalties(
        graph: &'a CompactGraph,
        start: VirtualEndpoint,
        goal: VirtualEndpoint,
        profile: RouteProfile,
        penalties: Vec<f64>,
    ) -> Self {
        RouteRequest {
            graph,
            start,
            goal,
            profile,
            cost: EdgeCostProvider::new(profile),
            edge_penalties: penalties,
        }
    }

    /// 边时间成本（metrics/ETA）。
    fn edge_time(&self, eid: u32) -> f64 {
        self.cost.edge_time(self.graph, eid)
    }

    /// 边速度（m/s，虚拟段成本用）。
    fn speed_of(&self, eid: u32) -> f64 {
        self.cost.edge_speed(self.graph, eid)
    }

    /// 全图速度上界（A* 下界启发用；取 cap 与各 profile 速度最大值）。
    fn vmax(&self) -> f64 {
        self.cost.params.free_flow_cap_kph / 3.6
    }

    fn w_time(&self) -> f64 {
        self.cost.params.w_time
    }

    fn w_dist(&self) -> f64 {
        self.cost.params.w_dist
    }
}

/// 求解器：预分配工作数组（§74 generation counter，避免每搜清空）。
pub struct Router {
    g_score: Vec<f64>,
    gen: Vec<u32>,
    parent_edge: Vec<u32>,
    generation: u32,
}

impl Router {
    pub fn new(node_count: usize) -> Self {
        Router {
            g_score: vec![f64::INFINITY; node_count],
            gen: vec![0; node_count],
            parent_edge: vec![u32::MAX; node_count],
            generation: 1,
        }
    }

    /// Dijkstra reference（§70）。
    pub fn dijkstra(&mut self, req: &RouteRequest) -> Option<Route> {
        self.search(req, false)
    }

    /// A*（§72）。
    pub fn astar(&mut self, req: &RouteRequest) -> Option<Route> {
        self.search(req, true)
    }

    /// 内部搜索（astar 开关）。
    fn search(&mut self, req: &RouteRequest, astar: bool) -> Option<Route> {
        let g = req.graph;
        let gen = self.generation;
        self.generation += 1;
        if self.generation == u32::MAX {
            self.generation = 1;
        }
        let mut heap: BinaryHeap<Reverse<(F, u32)>> = BinaryHeap::new();

        // —— 起点展开：forward 段 → to 节点；backward 段 → from 节点（§57）——
        let start_e = &g.edges[req.start.edge_id as usize];
        let mut pushed_start = false;
        if req.start.allow_forward {
            let seg = seg_cost(req, req.start.edge_id, req.start.offset, true);
            self.gen[start_e.to as usize] = gen;
            self.g_score[start_e.to as usize] = seg;
            self.parent_edge[start_e.to as usize] = u32::MAX;
            let h = if astar {
                heuristic(g, start_e.to, req)
            } else {
                0.0
            };
            heap.push(Reverse((F(seg + h), start_e.to)));
            pushed_start = true;
        }
        // offset≈0 时车辆已在 from 节点：backward 分支成本 0，总是允许（非回头）。
        if req.start.allow_backward || req.start.offset <= 1e-6 {
            let seg = seg_cost(req, req.start.edge_id, req.start.offset, false);
            if self.gen[start_e.from as usize] != gen {
                self.gen[start_e.from as usize] = gen;
                self.g_score[start_e.from as usize] = seg;
                self.parent_edge[start_e.from as usize] = u32::MAX;
                let h = if astar {
                    heuristic(g, start_e.from, req)
                } else {
                    0.0
                };
                heap.push(Reverse((F(seg + h), start_e.from)));
            } else if seg < self.g_score[start_e.from as usize] {
                self.g_score[start_e.from as usize] = seg;
                let h = if astar {
                    heuristic(g, start_e.from, req)
                } else {
                    0.0
                };
                heap.push(Reverse((F(seg + h), start_e.from)));
            }
            pushed_start = true;
        }
        debug_assert!(pushed_start, "起点必须至少允许一个方向");

        // —— 起点 edge == 终点 edge：单段路线（无需搜索）——
        let goal_e = &g.edges[req.goal.edge_id as usize];
        if req.start.edge_id == req.goal.edge_id {
            return Some(single_edge_route(req));
        }

        // —— 主循环 ——
        let mut goal_node: Option<u32> = None;
        let mut goal_fwd = true;
        while let Some(Reverse((f, node))) = heap.pop() {
            let f = f.0;
            if self.gen[node as usize] != gen {
                continue; // 过期条目
            }
            let h_cur = if astar { heuristic(g, node, req) } else { 0.0 };
            if f > self.g_score[node as usize] + h_cur + 1e-6 {
                continue; // 已改进
            }
            // 目标判定：node 为 goal edge 的 from（forward 段到达）或 to（backward 段）
            if node == goal_e.from && req.goal.allow_forward {
                goal_node = Some(node);
                goal_fwd = true;
                break;
            }
            if node == goal_e.to && req.goal.allow_backward {
                goal_node = Some(node);
                goal_fwd = false;
                break;
            }
            // 扩展出边
            let gn = self.g_score[node as usize];
            for &eid in g.out_edges(node as usize) {
                let e = &g.edges[eid as usize];
                let nf = gn + req.edge_cost(eid);
                let en = e.to;
                if self.gen[en as usize] != gen {
                    self.gen[en as usize] = gen;
                    self.g_score[en as usize] = nf;
                    self.parent_edge[en as usize] = eid;
                    let h = if astar { heuristic(g, en, req) } else { 0.0 };
                    heap.push(Reverse((F(nf + h), en)));
                } else if nf + 1e-6 < self.g_score[en as usize] {
                    self.g_score[en as usize] = nf;
                    self.parent_edge[en as usize] = eid;
                    let h = if astar { heuristic(g, en, req) } else { 0.0 };
                    heap.push(Reverse((F(nf + h), en)));
                }
            }
        }
        let goal_node = goal_node?;

        // —— 回溯构造边序列 ——
        let mut edges = Vec::new();
        let mut n = goal_node;
        let mut guard = 0u32;
        while self.parent_edge[n as usize] != u32::MAX && guard < 10_000_000 {
            let eid = self.parent_edge[n as usize];
            edges.push(eid);
            n = g.edges[eid as usize].from;
            guard += 1;
        }
        edges.reverse();

        // —— 虚拟段 ——
        // offset≈0 时 backward 分支总是展开（成本 0）；路径从 start_e.from 出发则起点为原地。
        let start_virtual = if (req.start.allow_backward || req.start.offset <= 1e-6)
            && edges
                .first()
                .is_some_and(|&e| g.edges[e as usize].from == start_e.from)
        {
            Some((req.start.edge_id, req.start.offset, false))
        } else if req.start.allow_forward {
            Some((req.start.edge_id, req.start.offset, true))
        } else {
            None
        };
        let end_virtual = if goal_fwd {
            Some((req.goal.edge_id, req.goal.offset, true))
        } else {
            Some((req.goal.edge_id, req.goal.offset, false))
        };

        // —— metrics（§77）——
        let mut dist = 0.0;
        let mut eta = 0.0;
        let mut road = 0u32;
        let mut jct = 0u32;
        let mut sig = 0u32;
        let mut ferry = 0u32;
        let mut train = 0u32;
        let mut gps_avoid = 0.0;
        let mut unknown = 0.0;
        for &eid in &edges {
            let e = &g.edges[eid as usize];
            let len = edge_len(g, eid);
            dist += len;
            eta += req.edge_time(eid);
            match e.kind {
                nav_graph::EdgeKind::Road => road += 1,
                nav_graph::EdgeKind::JunctionMovement => {
                    jct += 1;
                    if e.semaphore_id >= 0 {
                        sig += 1;
                    }
                }
                nav_graph::EdgeKind::Ferry => ferry += 1,
                nav_graph::EdgeKind::Train => train += 1,
                nav_graph::EdgeKind::ServiceAccess => {}
            }
            if e.gps_avoid() {
                gps_avoid += len;
            }
            if e.kind == nav_graph::EdgeKind::Road && e.speed_limit < 0 {
                unknown += len;
            }
        }
        if let Some((eid, offset, fwd)) = start_virtual {
            let len = edge_len(g, eid);
            let seg = if fwd { len - offset } else { offset };
            dist += seg;
            eta += seg_cost(req, eid, offset, fwd);
        }
        if let Some((eid, offset, fwd)) = end_virtual {
            let len = edge_len(g, eid);
            let seg = if fwd { offset } else { len - offset };
            dist += seg;
            eta += seg_cost(req, eid, offset, fwd);
        }

        Some(Route {
            profile: req.profile,
            edges,
            start_virtual,
            end_virtual,
            distance_m: dist,
            eta_s: eta,
            road_edge_count: road,
            junction_count: jct,
            signal_count: sig,
            ferry_count: ferry,
            train_count: train,
            gps_avoid_distance: gps_avoid,
            unknown_speed_distance: unknown,
        })
    }
}

/// 虚拟段成本（按 profile：距离 / 时间 / 加权）。
fn seg_cost(req: &RouteRequest, edge_id: u32, offset: f64, forward: bool) -> f64 {
    let len = edge_len(req.graph, edge_id);
    let seg = if forward { len - offset } else { offset };
    match req.profile {
        RouteProfile::Shortest => seg,
        RouteProfile::Fastest => seg / req.speed_of(edge_id),
        RouteProfile::Balanced => {
            req.w_time() * seg / req.speed_of(edge_id) + req.w_dist() * seg / 1000.0
        }
    }
}

/// 起点 edge == 终点 edge：单段路线。
fn single_edge_route(req: &RouteRequest) -> Route {
    let g = req.graph;
    let s = req.start.offset;
    let go = req.goal.offset;
    let fwd = go >= s;
    let dist = (go - s).abs();
    // 审查修复：ETA 必须与实际行驶段一致（fwd: s→go 的 (go-s) 段；backward: go→s 的 (s-go) 段）
    let seg = if fwd {
        seg_cost(req, req.start.edge_id, s, true) - seg_cost(req, req.start.edge_id, go, true)
    } else {
        seg_cost(req, req.start.edge_id, go, false) - seg_cost(req, req.start.edge_id, s, false)
    }
    .abs();
    let e = &g.edges[req.start.edge_id as usize];
    Route {
        profile: req.profile,
        edges: vec![],
        start_virtual: Some((req.start.edge_id, s, fwd)),
        end_virtual: Some((req.goal.edge_id, go, fwd)),
        distance_m: dist,
        eta_s: seg,
        road_edge_count: (e.kind == nav_graph::EdgeKind::Road) as u32,
        junction_count: (e.kind == nav_graph::EdgeKind::JunctionMovement) as u32,
        signal_count: (e.kind == nav_graph::EdgeKind::JunctionMovement && e.semaphore_id >= 0)
            as u32,
        ferry_count: (e.kind == nav_graph::EdgeKind::Ferry) as u32,
        train_count: (e.kind == nav_graph::EdgeKind::Train) as u32,
        gps_avoid_distance: if e.gps_avoid() { dist } else { 0.0 },
        unknown_speed_distance: if e.kind == nav_graph::EdgeKind::Road && e.speed_limit < 0 {
            dist
        } else {
            0.0
        },
    }
}

/// 边长度（几何优先，空几何回退欧氏）。
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

/// A* 启发（§72/73）：必须 h ≤ h*（下界）。
/// Shortest：欧氏距离；Fastest：欧氏 / v_max；Balanced：加权组合（下界的线性组合仍为下界）。
fn heuristic(g: &CompactGraph, node: u32, req: &RouteRequest) -> f64 {
    let a = g.positions[node as usize];
    let goal_e = &g.edges[req.goal.edge_id as usize];
    let b = g.positions[goal_e.from as usize];
    let c = g.positions[goal_e.to as usize];
    let d1 = ((a.0 - b.0).powi(2) + (a.2 - b.2).powi(2)).sqrt();
    let d2 = ((a.0 - c.0).powi(2) + (a.2 - c.2).powi(2)).sqrt();
    let d = d1.min(d2);
    match req.profile {
        RouteProfile::Shortest => d,
        RouteProfile::Fastest => d / req.vmax(),
        RouteProfile::Balanced => req.w_time() * d / req.vmax() + req.w_dist() * d / 1000.0,
    }
}

/// 整条路线的总成本（A* == Dijkstra 回归用；§71）。
pub fn route_cost(g: &CompactGraph, route: &Route, profile: RouteProfile) -> f64 {
    let cost = EdgeCostProvider::new(profile);
    let mut c = 0.0;
    for &eid in &route.edges {
        c += cost.edge_cost(g, eid);
    }
    // 虚拟段（近似：距离按速度折算）
    if let Some((eid, offset, fwd)) = route.start_virtual {
        let len = edge_len(g, eid);
        let seg = if fwd { len - offset } else { offset };
        c += match profile {
            RouteProfile::Shortest => seg,
            RouteProfile::Fastest => seg / cost.edge_speed(g, eid),
            RouteProfile::Balanced => {
                cost.params.w_time * seg / cost.edge_speed(g, eid)
                    + cost.params.w_dist * seg / 1000.0
            }
        };
    }
    if let Some((eid, offset, fwd)) = route.end_virtual {
        let len = edge_len(g, eid);
        let seg = if fwd { offset } else { len - offset };
        c += match profile {
            RouteProfile::Shortest => seg,
            RouteProfile::Fastest => seg / cost.edge_speed(g, eid),
            RouteProfile::Balanced => {
                cost.params.w_time * seg / cost.edge_speed(g, eid)
                    + cost.params.w_dist * seg / 1000.0
            }
        };
    }
    c
}
