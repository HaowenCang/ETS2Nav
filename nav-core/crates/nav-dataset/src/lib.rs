// nav-dataset：ETS2Nav Dataset v2 加载器（routing.graph / junction.graph / manifest）。
// 独立于 C# Map Compiler 实现；版本策略：support version == expected（DatasetVersionMismatch 拒绝）。
// P2-navigation-core-plan.md §26-30。
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

pub const EXPECTED_DATASET_VERSION: u32 = 2;
const ROUTING_MAGIC: &[u8; 7] = b"ETS2RG1";
const JUNCTION_MAGIC: &[u8; 7] = b"ETS2JG1";
const EXPECTED_ENDIANNESS: u32 = 0x12345678;

#[derive(Debug)]
pub enum DatasetError {
    Io(std::io::Error),
    /// 非 IO 解析/校验错误（含 sqlite 访问）。
    Other(String),
    Corrupt(String),
    VersionMismatch {
        found: u32,
        expected: u32,
    },
    MissingFile(String),
    Manifest(String),
}

impl std::fmt::Display for DatasetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatasetError::Io(e) => write!(f, "IO: {e}"),
            DatasetError::Other(s) => write!(f, "Other: {s}"),
            DatasetError::Corrupt(m) => write!(f, "corrupt dataset: {m}"),
            DatasetError::VersionMismatch { found, expected } => {
                write!(
                    f,
                    "dataset version mismatch: found {found}, expected {expected}"
                )
            }
            DatasetError::MissingFile(m) => write!(f, "missing file: {m}"),
            DatasetError::Manifest(m) => write!(f, "manifest: {m}"),
        }
    }
}

impl From<std::io::Error> for DatasetError {
    fn from(e: std::io::Error) -> Self {
        DatasetError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, DatasetError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Road = 0,
    JunctionMovement = 1,
    Ferry = 2,
    Train = 3,
    ServiceAccess = 4,
}

impl EdgeKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(EdgeKind::Road),
            1 => Some(EdgeKind::JunctionMovement),
            2 => Some(EdgeKind::Ferry),
            3 => Some(EdgeKind::Train),
            4 => Some(EdgeKind::ServiceAccess),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub uid: u64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: u32,
    pub to: u32,
    pub kind: EdgeKind,
    pub length: f32,
    pub source_uid: u64,
    /// 世界坐标 polyline（固定点坐标 1/256 已还原为米）。len>=2；空 = 无几何。
    pub geometry: Vec<(f64, f64, f64)>,
    /// -1 = 未知 / 0 = 无限速 / >0 = km/h
    pub speed_limit: i16,
    /// 0=unknown 1=local 2=expressway 3=motorway
    pub road_class: u8,
    pub semaphore_id: i32,
    pub flags: u8,
    pub movement_id: Option<u32>,
}

impl Edge {
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

#[derive(Debug)]
pub struct RoutingGraph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone)]
pub struct JunctionMovement {
    pub id: u32,
    pub entry: u64,
    pub exit: u64,
    pub length: f32,
    pub turn_type: i8,
    pub semaphore_id: i32,
    pub signal_group_type: String,
    pub geometry: Vec<(f64, f64, f64)>,
}

#[derive(Debug)]
pub struct Junction {
    pub uid: u64,
    pub prefab_token: String,
    pub node_uids: Vec<u64>,
    pub movements: Vec<JunctionMovement>,
}

#[derive(Debug)]
pub struct JunctionGraph {
    pub junctions: Vec<Junction>,
}

/// 加载数据集目录（routing.graph + junction.graph + manifest 校验）。
pub fn load_dataset(dir: &Path) -> Result<(RoutingGraph, JunctionGraph)> {
    let manifest_path = dir.join("manifest.json");
    if !manifest_path.exists() {
        return Err(DatasetError::MissingFile("manifest.json".into()));
    }
    check_manifest(&manifest_path)?;
    let routing = load_routing_graph(&dir.join("routing.graph"))?;
    let junction = load_junction_graph(&dir.join("junction.graph"))?;
    Ok((routing, junction))
}

fn check_manifest(path: &Path) -> Result<()> {
    let text = std::fs::read_to_string(path).map_err(DatasetError::Io)?;
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| DatasetError::Manifest(format!("json 解析失败: {e}")))?;
    let dv = v
        .get("dataset_version")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| DatasetError::Manifest("缺少 dataset_version".into()))?;
    if dv != EXPECTED_DATASET_VERSION as u64 {
        return Err(DatasetError::VersionMismatch {
            found: dv as u32,
            expected: EXPECTED_DATASET_VERSION,
        });
    }
    Ok(())
}

fn read_all(path: &Path) -> Result<Vec<u8>> {
    let f = File::open(path)?;
    let mut r = BufReader::new(f);
    let mut buf = Vec::new();
    r.read_to_end(&mut buf)?;
    Ok(buf)
}

fn check_header(data: &[u8], magic: &[u8; 7], what: &str) -> Result<(u32, u32, usize)> {
    if data.len() < 23 {
        return Err(DatasetError::Corrupt(format!(
            "{what}: 文件过小 {}",
            data.len()
        )));
    }
    if &data[0..7] != magic {
        return Err(DatasetError::Corrupt(format!("{what}: magic 不符")));
    }
    let version = u32_le(&data[7..11]);
    if version != EXPECTED_DATASET_VERSION {
        return Err(DatasetError::VersionMismatch {
            found: version,
            expected: EXPECTED_DATASET_VERSION,
        });
    }
    if u32_le(&data[11..15]) != EXPECTED_ENDIANNESS {
        return Err(DatasetError::Corrupt(format!("{what}: endianness 不符")));
    }
    Ok((version, u32_le(&data[15..19]), 19))
}

/// routing.graph v2：magic(7)+ver(4)+endian(4)+node_count(4)+edge_count(4)+geom_point_count(4)
/// nodes 20B；edges 35B(+4B movement_id)；geometry 12B/点。
pub fn load_routing_graph(path: &Path) -> Result<RoutingGraph> {
    let data = read_all(path)?;
    let (_, _, _) = check_header(&data, ROUTING_MAGIC, "routing.graph")?;
    let node_count = u32_le(&data[15..19]) as usize;
    let edge_count = u32_le(&data[19..23]) as usize;
    let geom_point_count = u32_le(&data[23..27]) as usize;

    // pass 1：扫描边记录区（35B + 可选 movement_id 4B）确定 geometry 区起点
    let mut scan = 27 + node_count * 20;
    if scan > data.len() {
        return Err(DatasetError::Corrupt("routing.graph: 节点区越界".into()));
    }
    for i in 0..edge_count {
        if scan + 35 > data.len() {
            return Err(DatasetError::Corrupt(format!(
                "routing.graph: edge {i} 越界"
            )));
        }
        let flags = data[scan + 34];
        scan += 35 + if flags & 8 != 0 { 4 } else { 0 };
    }
    let geom_base = scan;
    if geom_base + geom_point_count * 12 != data.len() {
        return Err(DatasetError::Corrupt(
            "routing.graph: 尾部偏移不符（geometry 区长度）".into(),
        ));
    }

    // 解析 nodes
    let mut nodes = Vec::with_capacity(node_count);
    let mut pos = 27;
    for _ in 0..node_count {
        let uid = u64_le(&data[pos..pos + 8]);
        let x = i32_le(&data[pos + 8..pos + 12]) as f64 / 256.0;
        let y = i32_le(&data[pos + 12..pos + 16]) as f64 / 256.0;
        let z = i32_le(&data[pos + 16..pos + 20]) as f64 / 256.0;
        nodes.push(Node { uid, x, y, z });
        pos += 20;
    }

    // 解析 edges（含几何引用，从 geom_base 读点）
    let mut edges = Vec::with_capacity(edge_count);
    for i in 0..edge_count {
        let from = u32_le(&data[pos..pos + 4]);
        let to = u32_le(&data[pos + 4..pos + 8]);
        let kind = EdgeKind::from_u8(data[pos + 8])
            .ok_or_else(|| DatasetError::Corrupt(format!("edge {i}: kind 非法")))?;
        let length = f32_le(&data[pos + 9..pos + 13]);
        let source_uid = u64_le(&data[pos + 13..pos + 21]);
        let geom_offset = u32_le(&data[pos + 21..pos + 25]) as usize;
        let geom_count = u16_le(&data[pos + 25..pos + 27]) as usize;
        let speed_limit = i16_le(&data[pos + 27..pos + 29]);
        let road_class = data[pos + 29];
        let semaphore_id = i32_le(&data[pos + 30..pos + 34]);
        let flags = data[pos + 34];
        if from as usize >= node_count || to as usize >= node_count {
            return Err(DatasetError::Corrupt(format!("edge {i}: 节点索引越界")));
        }
        if !geom_offset.is_multiple_of(3) || (geom_count != 0 && geom_count < 2) {
            return Err(DatasetError::Corrupt(format!("edge {i}: 几何索引异常")));
        }
        if geom_offset / 3 + geom_count > geom_point_count {
            return Err(DatasetError::Corrupt(format!("edge {i}: 几何越界")));
        }
        if speed_limit < -1 || road_class > 3 {
            return Err(DatasetError::Corrupt(format!("edge {i}: speed/class 非法")));
        }
        pos += 35;
        let movement_id = if flags & 8 != 0 {
            let mv = u32_le(&data[pos..pos + 4]);
            pos += 4;
            Some(mv)
        } else {
            None
        };
        let mut geometry = Vec::with_capacity(geom_count);
        let gp = geom_offset / 3;
        for k in 0..geom_count {
            let o = geom_base + (gp + k) * 12;
            let x = i32_le(&data[o..o + 4]) as f64 / 256.0;
            let y = i32_le(&data[o + 4..o + 8]) as f64 / 256.0;
            let z = i32_le(&data[o + 8..o + 12]) as f64 / 256.0;
            geometry.push((x, y, z));
        }
        edges.push(Edge {
            from,
            to,
            kind,
            length,
            source_uid,
            geometry,
            speed_limit,
            road_class,
            semaphore_id,
            flags,
            movement_id,
        });
    }
    Ok(RoutingGraph { nodes, edges })
}

/// junction.graph v2：magic(7)+ver(4)+endian(4)+junction_count(4)
/// junction: uid 8 + token 64 + node_count u8 + node_uids + movement_count u32
/// movement: id 4 + entry 8 + exit 8 + length 4 + turn 1 + sid 4 + gt_len 1 + gt + off 4 + cnt 2
pub fn load_junction_graph(path: &Path) -> Result<JunctionGraph> {
    let data = read_all(path)?;
    let (_, _, _) = check_header(&data, JUNCTION_MAGIC, "junction.graph")?;
    let junction_count = u32_le(&data[15..19]) as usize;
    let mut junctions = Vec::with_capacity(junction_count);
    // 几何区定位：先扫 junction 区得到总点数
    let (mut scan, mut total_geom_points) = (19usize, 0usize);
    for _ in 0..junction_count {
        if scan + 73 > data.len() {
            return Err(DatasetError::Corrupt(
                "junction.graph: junction 头越界".into(),
            ));
        }
        let nc = data[scan + 72] as usize;
        scan += 73 + nc * 8;
        if scan + 4 > data.len() {
            return Err(DatasetError::Corrupt(
                "junction.graph: movement_count 越界".into(),
            ));
        }
        let mc = u32_le(&data[scan..scan + 4]) as usize;
        scan += 4;
        for _ in 0..mc {
            if scan + 36 > data.len() {
                return Err(DatasetError::Corrupt(
                    "junction.graph: movement 越界".into(),
                ));
            }
            let gtl = data[scan + 29] as usize;
            let cnt = u16_le(&data[scan + 34 + gtl..scan + 36 + gtl]) as usize;
            scan += 36 + gtl;
            total_geom_points += cnt;
        }
    }
    let geom_base = scan;
    if geom_base + total_geom_points * 12 != data.len() {
        return Err(DatasetError::Corrupt("junction.graph: 尾部偏移不符".into()));
    }
    let mut pos = 19;
    for i in 0..junction_count {
        let uid = u64_le(&data[pos..pos + 8]);
        let token = String::from_utf8_lossy(&data[pos + 8..pos + 72])
            .trim_end_matches('\0')
            .to_string();
        let nc = data[pos + 72] as usize;
        pos += 73;
        let mut node_uids = Vec::with_capacity(nc);
        for _ in 0..nc {
            node_uids.push(u64_le(&data[pos..pos + 8]));
            pos += 8;
        }
        let mc = u32_le(&data[pos..pos + 4]) as usize;
        pos += 4;
        let mut movements = Vec::with_capacity(mc);
        for _ in 0..mc {
            let id = u32_le(&data[pos..pos + 4]);
            let entry = u64_le(&data[pos + 4..pos + 12]);
            let exit = u64_le(&data[pos + 12..pos + 20]);
            let length = f32_le(&data[pos + 20..pos + 24]);
            let turn_type = data[pos + 24] as i8;
            let semaphore_id = i32_le(&data[pos + 25..pos + 29]);
            let gtl = data[pos + 29] as usize;
            let signal_group_type =
                String::from_utf8_lossy(&data[pos + 30..pos + 30 + gtl]).to_string();
            let geom_offset = u32_le(&data[pos + 30 + gtl..pos + 34 + gtl]) as usize;
            let geom_count = u16_le(&data[pos + 34 + gtl..pos + 36 + gtl]) as usize;
            let gp = geom_offset / 3;
            let mut geometry = Vec::with_capacity(geom_count);
            for k in 0..geom_count {
                let o = geom_base + (gp + k) * 12;
                let x = i32_le(&data[o..o + 4]) as f64 / 256.0;
                let y = i32_le(&data[o + 4..o + 8]) as f64 / 256.0;
                let z = i32_le(&data[o + 8..o + 12]) as f64 / 256.0;
                geometry.push((x, y, z));
            }
            movements.push(JunctionMovement {
                id,
                entry,
                exit,
                length,
                turn_type,
                semaphore_id,
                signal_group_type,
                geometry,
            });
            pos += 36 + gtl;
        }
        junctions.push(Junction {
            uid,
            prefab_token: token,
            node_uids,
            movements,
        });
        let _ = i;
    }
    Ok(JunctionGraph { junctions })
}

fn u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn u64_le(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}
fn i32_le(b: &[u8]) -> i32 {
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn i16_le(b: &[u8]) -> i16 {
    i16::from_le_bytes([b[0], b[1]])
}
fn u16_le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn f32_le(b: &[u8]) -> f32 {
    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// POI 记录（search.db poi 表）。
#[derive(Debug, Clone)]
pub struct PoiRecord {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub x: f64,
    pub z: f64,
    pub access_node_hex: String,
}

/// 从 search.db 加载全部 POI（P2-15 Destination Resolver 用）。
pub fn load_pois(search_db: &std::path::Path) -> std::result::Result<Vec<PoiRecord>, DatasetError> {
    let conn = rusqlite::Connection::open(search_db)
        .map_err(|e| DatasetError::Other(format!("打开 search.db: {e}")))?;
    let mut stmt = conn
        .prepare("SELECT id, type, name, x, z, access_node FROM poi")
        .map_err(|e| DatasetError::Other(format!("查询 poi 表: {e}")))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(PoiRecord {
                id: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                x: row.get(3)?,
                z: row.get(4)?,
                access_node_hex: row.get(5)?,
            })
        })
        .map_err(|e| DatasetError::Other(format!("读取 poi 行: {e}")))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| DatasetError::Other(format!("POI 解析: {e}")))?);
    }
    Ok(out)
}
