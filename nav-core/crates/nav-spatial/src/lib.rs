// nav-spatial：edge bounding-box 空间索引（Map Matching 候选查询，P2 计划 §42-44）。
// 实现选择：统一网格（cell 哈希）而非完整 R-tree——静态数据集上等价查询语义
// （覆盖 cell 集合 = R-tree 节点范围），构建 O(E)、查询 O(cells+候选)，确定性。
// ponytail: 若后续查询 p99 超目标（10ms）或内存超标，升级 R-tree/STR 打包（P2-19 性能包）。
use nav_graph::{CompactGraph, EdgeKind};
use std::collections::HashMap;

pub const DEFAULT_CELL_SIZE: f64 = 256.0; // 米（城市路口密度下候选有限）

pub struct SpatialIndex {
    cell_size: f64,
    inv_cell: f64,
    /// cell (cx,cz) → 穿过该 cell 的 CSR 边 id 列表。
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// 每边的 bbox（CSR 边序）：(min_x, min_z, max_x, max_z)；ferry/train 不入索引。
    bboxes: Vec<Option<(f64, f64, f64, f64)>>,
}

impl SpatialIndex {
    /// 构建：Road + JunctionMovement 边按 polyline bbox 插入覆盖 cells。
    pub fn build(graph: &CompactGraph, cell_size: f64) -> Self {
        let mut idx = SpatialIndex {
            cell_size,
            inv_cell: 1.0 / cell_size,
            cells: HashMap::new(),
            bboxes: vec![None; graph.edges.len()],
        };
        for (eid, e) in graph.edges.iter().enumerate() {
            if e.kind != EdgeKind::Road && e.kind != EdgeKind::JunctionMovement {
                continue;
            }
            let pts = graph.edge_geometry(e);
            if pts.is_empty() {
                // 无几何：用端点（positions）
                let a = graph.positions[e.from as usize];
                let b = graph.positions[e.to as usize];
                idx.insert_edge(eid, a.0, a.2, b.0, b.2);
                continue;
            }
            let (mut minx, mut minz, mut maxx, mut maxz) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for (x, _, z) in pts {
                minx = minx.min(*x);
                maxx = maxx.max(*x);
                minz = minz.min(*z);
                maxz = maxz.max(*z);
            }
            idx.insert_edge(eid, minx, minz, maxx, maxz);
        }
        idx
    }

    fn insert_edge(&mut self, eid: usize, minx: f64, minz: f64, maxx: f64, maxz: f64) {
        self.bboxes[eid] = Some((minx, minz, maxx, maxz));
        let (c0x, c0z) = self.cell_of(minx, minz);
        let (c1x, c1z) = self.cell_of(maxx, maxz);
        for cx in c0x..=c1x {
            for cz in c0z..=c1z {
                self.cells.entry((cx, cz)).or_default().push(eid as u32);
            }
        }
    }

    #[inline]
    fn cell_of(&self, x: f64, z: f64) -> (i32, i32) {
        (
            x.mul_add(self.inv_cell, 0.0).floor() as i32,
            z.mul_add(self.inv_cell, 0.0).floor() as i32,
        )
    }

    /// 半径查询：返回候选 CSR 边 id（bbox 与圆相交的边；去重）。
    /// 注意：返回的是**粗候选**（bbox 相交），精确点到 polyline 距离由匹配器计算。
    pub fn query_radius(&self, x: f64, z: f64, radius: f64) -> Vec<u32> {
        let r = radius + self.cell_size; // 容差：cell 边界 + 半径
        let (c0x, c0z) = self.cell_of(x - r, z - r);
        let (c1x, c1z) = self.cell_of(x + r, z + r);
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let r2 = radius * radius;
        for cx in c0x..=c1x {
            for cz in c0z..=c1z {
                let Some(ids) = self.cells.get(&(cx, cz)) else {
                    continue;
                };
                for &eid in ids {
                    if !seen.insert(eid) {
                        continue;
                    }
                    let Some((minx, minz, maxx, maxz)) = self.bboxes[eid as usize] else {
                        continue;
                    };
                    // bbox 与圆相交粗判：最近 bbox 点距离 ≤ radius
                    let nx = x.clamp(minx, maxx);
                    let nz = z.clamp(minz, maxz);
                    let dx = x - nx;
                    let dz = z - nz;
                    if dx * dx + dz * dz <= r2 {
                        out.push(eid);
                    }
                }
            }
        }
        out
    }

    /// 统计（CLI/测试用）。
    pub fn stats(&self) -> (usize, usize) {
        (self.bboxes.len(), self.cells.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nav_dataset::{Edge, EdgeKind, Node, RoutingGraph};
    use nav_graph::CompactGraph;

    fn sample() -> CompactGraph {
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
            Node {
                uid: 4,
                x: 1000.0,
                y: 0.0,
                z: 1000.0,
            },
        ];
        let mk = |from: u32, to: u32, kind: EdgeKind, geom: Vec<(f64, f64, f64)>| Edge {
            from,
            to,
            kind,
            length: 1.0,
            source_uid: 0,
            geometry: geom,
            speed_limit: 50,
            road_class: 1,
            semaphore_id: -1,
            flags: 0,
            movement_id: None,
        };
        let edges = vec![
            // 横边（y=0 线）：两点直线
            mk(
                0,
                1,
                EdgeKind::Road,
                vec![(0.0, 0.0, 0.0), (1000.0, 0.0, 0.0)],
            ),
            // 纵边
            mk(
                0,
                2,
                EdgeKind::Road,
                vec![(0.0, 0.0, 0.0), (0.0, 0.0, 1000.0)],
            ),
            // 对角弯曲边（中间点）
            mk(
                1,
                3,
                EdgeKind::JunctionMovement,
                vec![
                    (1000.0, 0.0, 0.0),
                    (700.0, 0.0, 300.0),
                    (400.0, 0.0, 600.0),
                    (0.0, 0.0, 1000.0),
                ],
            ),
            // 远边（不命中查询）
            mk(
                2,
                3,
                EdgeKind::Road,
                vec![(0.0, 0.0, 1000.0), (1000.0, 0.0, 1000.0)],
            ),
        ];
        CompactGraph::build(&RoutingGraph { nodes, edges })
    }

    #[test]
    fn radius_query_hits_nearby_edges() {
        let g = sample();
        let idx = SpatialIndex::build(&g, 256.0);
        // 原点附近：横边(0) + 纵边(1) + 对角边(2 中间点接近原点? 对角边最小 x/z=0/0 → 命中)
        let hits = idx.query_radius(10.0, 10.0, 50.0);
        assert!(hits.contains(&0), "横边应命中: {hits:?}");
        assert!(hits.contains(&1), "纵边应命中: {hits:?}");
        assert!(!hits.contains(&3), "远边不应命中: {hits:?}");
        // 远点查询
        let far = idx.query_radius(5000.0, 5000.0, 100.0);
        assert!(far.is_empty(), "远处应无候选: {far:?}");
    }

    #[test]
    fn ferry_edges_excluded() {
        let mut rg = sample_rg();
        rg.edges.push(Edge {
            from: 0,
            to: 3,
            kind: EdgeKind::Ferry,
            length: 1.0,
            source_uid: 0,
            geometry: vec![(0.0, 0.0, 0.0), (1000.0, 0.0, 1000.0)],
            speed_limit: -1,
            road_class: 0,
            semaphore_id: -1,
            flags: 0,
            movement_id: None,
        });
        let g = CompactGraph::build(&rg);
        let idx = SpatialIndex::build(&g, 256.0);
        let hits = idx.query_radius(500.0, 500.0, 500.0);
        assert!(
            !hits
                .iter()
                .any(|&e| g.edges[e as usize].kind == EdgeKind::Ferry),
            "ferry 不应入索引: {hits:?}"
        );
    }

    fn sample_rg() -> RoutingGraph {
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
            Node {
                uid: 4,
                x: 1000.0,
                y: 0.0,
                z: 1000.0,
            },
        ];
        let mk = |from: u32, to: u32, kind: EdgeKind| Edge {
            from,
            to,
            kind,
            length: 1.0,
            source_uid: 0,
            geometry: vec![(0.0, 0.0, 0.0), (1.0, 0.0, 1.0)],
            speed_limit: 50,
            road_class: 1,
            semaphore_id: -1,
            flags: 0,
            movement_id: None,
        };
        let edges = vec![
            mk(0, 1, EdgeKind::Road),
            mk(0, 2, EdgeKind::Road),
            mk(1, 3, EdgeKind::JunctionMovement),
        ];
        RoutingGraph { nodes, edges }
    }
}
