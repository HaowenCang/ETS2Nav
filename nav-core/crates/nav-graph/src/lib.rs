// nav-graph：Runtime 紧凑图（CSR / adjacency array）+ 节点压缩。
// P2-navigation-core-plan.md §19-20：只保留被边引用的节点；CSR 连续访问、确定性。
use nav_dataset::RoutingGraph;
pub use nav_dataset::{Edge, EdgeKind};

/// 压缩后的 CSR 出边图。
/// node_offsets[node]..node_offsets[node+1] 为 node 的出边在 edge_ids 中的范围；
/// edges[edge_ids[i]] 为边（自包含 from/to）。
pub struct CompactGraph {
    /// 活跃节点 uid（按原始索引序；被至少一条边引用的节点）。
    pub node_uids: Vec<u64>,
    /// 原始节点索引 → 压缩索引（未引用 = usize::MAX）。
    pub node_map: Vec<usize>,
    /// 压缩节点坐标（米）。
    pub positions: Vec<(f64, f64, f64)>,
    /// CSR 出边偏移（len = n_active + 1）。
    pub node_offsets: Vec<u32>,
    /// 出边 id 序列（按节点分组）。
    pub edge_ids: Vec<u32>,
    /// 边表（原始边 + 压缩端点索引）。
    pub edges: Vec<CompactEdge>,
    /// 反向邻接（入边）：in_offsets / in_edge_ids（指向 edges 索引）。
    pub in_offsets: Vec<u32>,
    pub in_edge_ids: Vec<u32>,
    /// 边几何点池（edges 的 geom_start/geom_len 引用；P2-05 spatial 用）。
    pub edges_geometry: Vec<(f64, f64, f64)>,
}

#[derive(Debug, Clone)]
pub struct CompactEdge {
    pub from: u32, // 压缩节点索引
    pub to: u32,   // 压缩节点索引
    pub kind: EdgeKind,
    pub length: f32,
    pub source_uid: u64,
    /// 几何点范围（引用 edges_geometry）。
    pub geom_start: u32,
    pub geom_len: u16, // 0 = 无几何
    pub speed_limit: i16,
    pub road_class: u8,
    pub semaphore_id: i32,
    pub flags: u8,
    pub movement_id: Option<u32>,
}

impl CompactEdge {
    pub fn no_ai(&self) -> bool {
        self.flags & 1 != 0
    }
    pub fn gps_avoid(&self) -> bool {
        self.flags & 2 != 0
    }
    pub fn secret(&self) -> bool {
        self.flags & 4 != 0
    }
}

impl CompactGraph {
    /// 从 dataset 构建压缩 CSR 图（节点压缩：仅保留被边引用的节点）。
    pub fn build(rg: &RoutingGraph) -> Self {
        let n_raw = rg.nodes.len();
        // 活跃节点标记（被任一 from/to 引用）
        let mut active = vec![false; n_raw];
        for e in &rg.edges {
            active[e.from as usize] = true;
            active[e.to as usize] = true;
        }
        let mut node_map = vec![usize::MAX; n_raw];
        let mut node_uids = Vec::new();
        let mut positions = Vec::new();
        for (i, n) in rg.nodes.iter().enumerate() {
            if active[i] {
                node_map[i] = node_uids.len();
                node_uids.push(n.uid);
                positions.push((n.x, n.y, n.z));
            }
        }
        let n = node_uids.len();
        // CSR 出边
        let mut degrees = vec![0u32; n];
        for e in &rg.edges {
            degrees[node_map[e.from as usize]] += 1;
        }
        let mut node_offsets = vec![0u32; n + 1];
        for i in 0..n {
            node_offsets[i + 1] = node_offsets[i] + degrees[i];
        }
        let mut cursor = node_offsets.clone();
        let mut edge_ids = vec![0u32; rg.edges.len()];
        let mut edges = Vec::with_capacity(rg.edges.len());
        let mut geometry: Vec<(f64, f64, f64)> = Vec::new();
        for (idx, e) in rg.edges.iter().enumerate() {
            let from = node_map[e.from as usize] as u32;
            let to = node_map[e.to as usize] as u32;
            let slot = cursor[from as usize];
            cursor[from as usize] += 1;
            edge_ids[slot as usize] = idx as u32;
            let geom_start = geometry.len() as u32;
            let geom_len = e.geometry.len().min(u16::MAX as usize) as u16;
            geometry.extend(e.geometry.iter().copied());
            edges.push(CompactEdge {
                from,
                to,
                kind: e.kind,
                length: e.length,
                source_uid: e.source_uid,
                geom_start,
                geom_len,
                speed_limit: e.speed_limit,
                road_class: e.road_class,
                semaphore_id: e.semaphore_id,
                flags: e.flags,
                movement_id: e.movement_id,
            });
        }
        // 反向邻接（入边）
        let mut in_degrees = vec![0u32; n];
        for e in &edges {
            in_degrees[e.to as usize] += 1;
        }
        let mut in_offsets = vec![0u32; n + 1];
        for i in 0..n {
            in_offsets[i + 1] = in_offsets[i] + in_degrees[i];
        }
        let mut in_cursor = in_offsets.clone();
        let mut in_edge_ids = vec![0u32; edges.len()];
        for (idx, e) in edges.iter().enumerate() {
            let slot = in_cursor[e.to as usize];
            in_cursor[e.to as usize] += 1;
            in_edge_ids[slot as usize] = idx as u32;
        }
        CompactGraph {
            node_uids,
            node_map,
            positions,
            node_offsets,
            edge_ids,
            edges,
            in_offsets,
            in_edge_ids,
            edges_geometry: geometry,
        }
    }

    /// 节点出边 id 范围。
    #[inline]
    pub fn out_edges(&self, node: usize) -> &[u32] {
        let a = self.node_offsets[node] as usize;
        let b = self.node_offsets[node + 1] as usize;
        &self.edge_ids[a..b]
    }

    /// 节点入边 id 范围。
    #[inline]
    pub fn in_edges(&self, node: usize) -> &[u32] {
        let a = self.in_offsets[node] as usize;
        let b = self.in_offsets[node + 1] as usize;
        &self.in_edge_ids[a..b]
    }

    /// 边几何（world polyline；空 = 无几何）。
    #[inline]
    pub fn edge_geometry(&self, e: &CompactEdge) -> &[(f64, f64, f64)] {
        if e.geom_len == 0 {
            return &[];
        }
        let a = e.geom_start as usize;
        &self.edges_geometry[a..a + e.geom_len as usize]
    }

    /// 节点 uid → 压缩索引。
    pub fn node_index(&self, uid: u64) -> Option<usize> {
        self.node_uids.iter().position(|&u| u == uid)
    }

    /// 活跃节点数。
    pub fn node_count(&self) -> usize {
        self.node_uids.len()
    }
}

/// 图统计（CLI 输出用）。
pub struct GraphStats {
    pub raw_nodes: usize,
    pub active_nodes: usize,
    pub edges: usize,
    pub road_edges: usize,
    pub movement_edges: usize,
    pub transit_edges: usize,
    pub geometry_points: usize,
    pub unknown_speed_edges: usize,
}

pub fn stats(rg: &RoutingGraph) -> GraphStats {
    let mut s = GraphStats {
        raw_nodes: rg.nodes.len(),
        active_nodes: 0,
        edges: rg.edges.len(),
        road_edges: 0,
        movement_edges: 0,
        transit_edges: 0,
        geometry_points: 0,
        unknown_speed_edges: 0,
    };
    for e in &rg.edges {
        match e.kind {
            EdgeKind::Road => s.road_edges += 1,
            EdgeKind::JunctionMovement => s.movement_edges += 1,
            EdgeKind::Ferry | EdgeKind::Train => s.transit_edges += 1,
            EdgeKind::ServiceAccess => {}
        }
        s.geometry_points += e.geometry.len();
        if e.speed_limit == -1 {
            s.unknown_speed_edges += 1;
        }
    }
    let c = CompactGraph::build(rg);
    s.active_nodes = c.node_count();
    s
}
