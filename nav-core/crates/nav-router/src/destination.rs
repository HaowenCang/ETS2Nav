// P2-15 Destination Resolver（§107-110）。
// 统一 Destination 模型（POI/Job/Map Click/Coordinate）；POI 用 search.db 的 access_node
// 作为正式导航目标（§108——不用 visual position）；Job → company POI（§109）；
// 解析失败 → NotFound（§110——不随意选同城另一家公司）。
use nav_graph::CompactGraph;
use nav_spatial::SpatialIndex;
use std::collections::HashMap;

use crate::snap::{snap_nearest, SnapPoint};

/// 目的地来源（§107）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestKind {
    Poi,
    Job,
    MapClick,
    Coordinate,
}

impl DestKind {
    pub fn name(&self) -> &'static str {
        match self {
            DestKind::Poi => "POI",
            DestKind::Job => "Job",
            DestKind::MapClick => "MapClick",
            DestKind::Coordinate => "Coordinate",
        }
    }
}

/// 解析错误（§110）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestError {
    NotFound,
    Ambiguous,
}

impl std::fmt::Display for DestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DestError::NotFound => write!(f, "DestinationNotFound"),
            DestError::Ambiguous => write!(f, "AmbiguousDestination"),
        }
    }
}

/// 解析后的导航目的地（§107：统一模型 + access snap）。
#[derive(Debug, Clone)]
pub struct Destination {
    pub kind: DestKind,
    pub name: String,
    /// 视觉位置（POI x/z；展示用）。
    pub position: (f64, f64, f64),
    /// 导航目标吸附点（POI 用 access_node 的节点位置——§108）。
    pub access_snap: SnapPoint,
}

/// POI 记录（复用 nav_dataset 定义）。
pub use nav_dataset::PoiRecord;

/// 目的地解析器（持有 POI 索引 + 全量节点位置——access_node 可能是孤立节点，
/// 被 CompactGraph 压缩剔除，但 POI 导航目标需要其世界位置（§108））。
pub struct DestinationResolver {
    pois: Vec<PoiRecord>,
    /// uid → (x, y, z)（全量节点，含孤立）。
    node_positions: HashMap<u64, (f64, f64, f64)>,
}

impl DestinationResolver {
    pub fn new(pois: Vec<PoiRecord>, all_nodes: &[nav_dataset::Node]) -> Self {
        let node_positions = all_nodes.iter().map(|n| (n.uid, (n.x, n.y, n.z))).collect();
        DestinationResolver {
            pois,
            node_positions,
        }
    }

    /// 按名称解析 POI（§108）：先精确、后包含（不区分大小写）；多命中 → Ambiguous。
    pub fn resolve_poi(
        &self,
        graph: &CompactGraph,
        spatial: &SpatialIndex,
        name: &str,
    ) -> Result<Destination, DestError> {
        let lower = name.to_lowercase();
        let mut exact: Vec<&PoiRecord> = Vec::new();
        let mut fuzzy: Vec<&PoiRecord> = Vec::new();
        for p in &self.pois {
            let pn = p.name.to_lowercase();
            if pn == lower {
                exact.push(p);
            } else if pn.contains(&lower) || lower.contains(&pn) {
                fuzzy.push(p);
            }
        }
        let cands = if !exact.is_empty() { exact } else { fuzzy };
        match cands.len() {
            0 => Err(DestError::NotFound),
            1 => self.to_destination(graph, spatial, cands[0], DestKind::Poi),
            _ => Err(DestError::Ambiguous),
        }
    }

    /// 按 access_node hex 精确解析（Job 目的地映射备用路径）。
    pub fn resolve_poi_by_access_node(
        &self,
        graph: &CompactGraph,
        spatial: &SpatialIndex,
        hex: &str,
    ) -> Option<Destination> {
        let h = hex.to_lowercase();
        self.pois
            .iter()
            .find(|p| p.access_node_hex.to_lowercase() == h)
            .and_then(|p| self.to_destination(graph, spatial, p, DestKind::Poi).ok())
    }

    /// Job 目的地（§109）：城市 + 公司名 → Company 类 POI。
    /// 匹配：type == Company + 名称相等（不区分大小写）；city_hint 过滤附近（25km 容差）；
    /// 多命中 → Ambiguous；无 → NotFound（§110）。
    pub fn resolve_job(
        &self,
        graph: &CompactGraph,
        spatial: &SpatialIndex,
        company: &str,
        city_hint: Option<&str>,
    ) -> Result<Destination, DestError> {
        let lower = company.to_lowercase();
        let mut cands: Vec<&PoiRecord> = self
            .pois
            .iter()
            .filter(|p| p.kind.eq_ignore_ascii_case("Company") && p.name.to_lowercase() == lower)
            .collect();
        if let Some(city) = city_hint {
            if let Ok(city_dest) = self.resolve_poi(graph, spatial, city) {
                let (cx, _, cz) = city_dest.position;
                cands.retain(|p| {
                    let dx = p.x - cx;
                    let dz = p.z - cz;
                    (dx * dx + dz * dz).sqrt() < 25_000.0
                });
            }
        }
        match cands.len() {
            0 => Err(DestError::NotFound),
            1 => self.to_destination(graph, spatial, cands[0], DestKind::Job),
            _ => Err(DestError::Ambiguous),
        }
    }

    /// 坐标/地图点击目的地（§107 Map Click/Coordinate）：snap 最近可路由边。
    pub fn resolve_coordinate(
        &self,
        graph: &CompactGraph,
        spatial: &SpatialIndex,
        x: f64,
        z: f64,
        kind: DestKind,
    ) -> Result<Destination, DestError> {
        let snap = snap_nearest(graph, spatial, x, z, 300.0).ok_or(DestError::NotFound)?;
        Ok(Destination {
            kind,
            name: format!("({x:.0},{z:.0})"),
            position: (x, 0.0, z),
            access_snap: snap,
        })
    }

    /// POI 记录 → Destination：access_node 为导航目标（§108）。
    fn to_destination(
        &self,
        graph: &CompactGraph,
        spatial: &SpatialIndex,
        p: &PoiRecord,
        kind: DestKind,
    ) -> Result<Destination, DestError> {
        let uid = u64::from_str_radix(&p.access_node_hex, 16).map_err(|_| DestError::NotFound)?;
        // 全量节点位置（access_node 可能是孤立节点——压缩图里没有）
        let pos = self.node_positions.get(&uid).ok_or(DestError::NotFound)?;
        let snap = snap_nearest(graph, spatial, pos.0, pos.2, 300.0).ok_or(DestError::NotFound)?;
        Ok(Destination {
            kind,
            name: p.name.clone(),
            position: (p.x, 0.0, p.z),
            access_snap: snap,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nav_dataset::{Edge, EdgeKind, Node, RoutingGraph};

    fn setup() -> (
        CompactGraph,
        SpatialIndex,
        DestinationResolver,
        Vec<nav_dataset::Node>,
    ) {
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
            speed_limit: 50,
            road_class: 1,
            semaphore_id: -1,
            flags: 0,
            movement_id: None,
        };
        let edges = vec![mk(0, 1), mk(0, 2)];
        let g = CompactGraph::build(&RoutingGraph {
            nodes: nodes.clone(),
            edges,
        });
        let sp = SpatialIndex::build(&g, 256.0);
        let pois = vec![
            PoiRecord {
                id: 1,
                kind: "Company".into(),
                name: "eurogoodies".into(),
                x: 300.0,
                z: 300.0,
                access_node_hex: "0000000000000002".into(),
            },
            PoiRecord {
                id: 2,
                kind: "City".into(),
                name: "berlin".into(),
                x: 500.0,
                z: 500.0,
                access_node_hex: "0000000000000003".into(),
            },
        ];
        (g, sp, DestinationResolver::new(pois, &nodes), nodes)
    }

    #[test]
    fn poi_resolves_via_access_node() {
        let (g, sp, r, _) = setup();
        let d = r.resolve_poi(&g, &sp, "eurogoodies").expect("应解析");
        assert_eq!(d.kind, DestKind::Poi);
        assert_eq!(d.name, "eurogoodies");
        // access_node = 节点 2（uid 2，位置 (1000,0)）→ snap 到边 0（0→1 直线）
        assert_eq!(d.access_snap.edge_id, 0, "应吸附到边 0");
    }

    #[test]
    fn job_resolves_company() {
        let (g, sp, r, _) = setup();
        let d = r
            .resolve_job(&g, &sp, "EUROGOODIES", Some("berlin"))
            .expect("应解析");
        assert_eq!(d.kind, DestKind::Job);
        // 城市 hint 过滤：berlin POI 在 (500,500)，company 在 (300,300)——< 25km ✓
        assert_eq!(d.name, "eurogoodies");
    }

    #[test]
    fn not_found_and_ambiguous() {
        let (g, sp, r, _) = setup();
        assert!(matches!(
            r.resolve_poi(&g, &sp, "nonesuch"),
            Err(DestError::NotFound)
        ));
        // 模糊：'e' 匹配多个（eurogoodies + berlin 含 e？——berlin 不含 'e' 的完整词……用 '0' 无匹配）
        // 构造模糊场景：名称 'e' 前缀匹配 eurogoodies 1 个——单命中；测试 Ambiguous 需两个相同
        let pois2 = vec![
            PoiRecord {
                id: 1,
                kind: "Company".into(),
                name: "dup".into(),
                x: 0.0,
                z: 0.0,
                access_node_hex: "0000000000000002".into(),
            },
            PoiRecord {
                id: 2,
                kind: "Company".into(),
                name: "dup".into(),
                x: 1000.0,
                z: 1000.0,
                access_node_hex: "0000000000000003".into(),
            },
        ];
        let r2 = DestinationResolver::new(
            pois2,
            &[
                nav_dataset::Node {
                    uid: 1,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                nav_dataset::Node {
                    uid: 2,
                    x: 1000.0,
                    y: 0.0,
                    z: 0.0,
                },
                nav_dataset::Node {
                    uid: 3,
                    x: 0.0,
                    y: 0.0,
                    z: 1000.0,
                },
            ],
        );
        assert!(matches!(
            r2.resolve_poi(&g, &sp, "dup"),
            Err(DestError::Ambiguous)
        ));
        // Job 无此公司
        assert!(matches!(
            r.resolve_job(&g, &sp, "nonesuch_co", None),
            Err(DestError::NotFound)
        ));
    }

    #[test]
    fn coordinate_resolves() {
        let (g, sp, r, _) = setup();
        let d = r
            .resolve_coordinate(&g, &sp, 100.0, 100.0, DestKind::MapClick)
            .expect("应解析");
        assert_eq!(d.kind, DestKind::MapClick);
        assert!(d.access_snap.lateral < 200.0);
    }
}
