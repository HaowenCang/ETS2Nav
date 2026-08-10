// P1-11 dataset-reader-smoke（P2-01 v2 升级）：Rust 独立读取 routing.graph / junction.graph。
// v2（ETS2NAV_DATASET_VERSION=2）：+ geometry table、+ speed_limit/road_class hot、
// + junction movement 显式 id。校验 magic/version/endianness/计数/范围/尾部偏移。
// 用法：dataset-reader-smoke <dataset-dir>
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

const ROUTING_MAGIC: &[u8; 7] = b"ETS2RG1";
const JUNCTION_MAGIC: &[u8; 7] = b"ETS2JG1";
const EXPECTED_ENDIANNESS: u32 = 0x12345678;
const EXPECTED_VERSION: u32 = 2;

fn main() {
    let dir = env::args().nth(1).unwrap_or_else(|| "berlin-dataset".to_string());
    let mut failures = 0;

    failures += smoke_routing(&Path::new(&dir).join("routing.graph"));
    failures += smoke_junction(&Path::new(&dir).join("junction.graph"));

    if failures == 0 {
        println!("dataset-reader-smoke: PASS");
    } else {
        println!("dataset-reader-smoke: FAIL ({failures} 项)");
        std::process::exit(1);
    }
}

fn read_all(path: &Path) -> Result<Vec<u8>, String> {
    let f = File::open(path).map_err(|e| format!("打开失败 {path:?}: {e}"))?;
    let mut r = BufReader::new(f);
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).map_err(|e| format!("读取失败: {e}"))?;
    Ok(buf)
}

fn smoke_routing(path: &Path) -> i32 {
    println!("== routing.graph ==");
    let data = match read_all(path) {
        Ok(d) => d,
        Err(e) => {
            println!("  FAIL {e}");
            return 1;
        }
    };
    if data.len() < 27 {
        println!("  FAIL 文件过小 {}", data.len());
        return 1;
    }
    let magic = &data[0..7];
    if magic != ROUTING_MAGIC {
        println!("  FAIL magic 不符: {:?}", String::from_utf8_lossy(magic));
        return 1;
    }
    let version = u32_le(&data[7..11]);
    let endianness = u32_le(&data[11..15]);
    let node_count = u32_le(&data[15..19]) as usize;
    let edge_count = u32_le(&data[19..23]) as usize;
    let geom_point_count = u32_le(&data[23..27]) as usize;
    println!(
        "  version={version} endianness=0x{endianness:x} nodes={node_count} edges={edge_count} geom_points={geom_point_count}"
    );
    if version != EXPECTED_VERSION {
        println!("  FAIL version 不符（期望 {EXPECTED_VERSION}）");
        return 1;
    }
    if endianness != EXPECTED_ENDIANNESS {
        println!("  FAIL endianness 不符");
        return 1;
    }
    // nodes: 20 B each；edges v2: 35 B + 4 B（movement_id 可选 flag bit3）；geometry: 12 B/点
    let nodes_bytes = node_count * 20;
    let mut pos = 27 + nodes_bytes;
    if pos > data.len() {
        println!("  FAIL 节点区越界");
        return 1;
    }
    let mut nonzero = 0;
    for i in 0..node_count.min(100) {
        let off = 27 + i * 20;
        let uid = u64_le(&data[off..off + 8]);
        if uid != 0 {
            nonzero += 1;
        }
    }
    let mut edge_ok = 0;
    let mut edge_kinds = BTreeMap::<u8, u32>::new();
    let mut geom_points_seen: usize = 0;
    let mut speed_sane = 0;
    let mut class_sane = 0;
    for i in 0..edge_count {
        if pos + 35 > data.len() {
            println!("  FAIL 边 {i} 越界");
            return 1;
        }
        let from = u32_le(&data[pos..pos + 4]);
        let to = u32_le(&data[pos + 4..pos + 8]);
        let kind = data[pos + 8];
        let geom_offset = u32_le(&data[pos + 21..pos + 25]);
        let geom_count = u16_le(&data[pos + 25..pos + 27]) as usize;
        let speed_limit = i16_le(&data[pos + 27..pos + 29]);
        let road_class = data[pos + 29];
        let semaphore_id = i32_le(&data[pos + 30..pos + 34]);
        let flags = data[pos + 34];
        if from >= node_count as u32 || to >= node_count as u32 {
            println!("  FAIL 边 {i} 节点越界 from={from} to={to}");
            return 1;
        }
        if geom_offset as usize % 3 != 0 {
            println!("  FAIL 边 {i} geom_offset 非 3 的倍数 offset={geom_offset}");
            return 1;
        }
        if geom_count != 0 && geom_count < 2 {
            println!("  FAIL 边 {i} geom_count 异常 count={geom_count}");
            return 1;
        }
        if (geom_offset as usize / 3) + geom_count > geom_point_count {
            println!("  FAIL 边 {i} 几何越界 offset={geom_offset} count={geom_count} total={geom_point_count}");
            return 1;
        }
        if speed_limit < -1 {
            println!("  FAIL 边 {i} speed_limit 非法 {speed_limit}");
            return 1;
        }
        if road_class > 3 {
            println!("  FAIL 边 {i} road_class 非法 {road_class}");
            return 1;
        }
        if semaphore_id >= 0 {
            // 带灯 movement 必须在 kind=JunctionMovement(1) 上（Road 无灯）
            if kind != 1 {
                println!("  FAIL 边 {i} Road/transit 带 semaphore_id={semaphore_id}");
                return 1;
            }
        }
        geom_points_seen += if geom_count == 0 { 2 } else { geom_count }; // 无几何按两端点计
        *edge_kinds.entry(kind).or_default() += 1;
        speed_sane += 1;
        class_sane += 1;
        pos += 35;
        if flags & 8 != 0 {
            pos += 4; // movement_id
        }
        edge_ok += 1;
    }
    if pos + geom_point_count * 12 != data.len() {
        println!(
            "  FAIL 尾部偏移不符 pos={pos} geom={geom_point_count} len={}",
            data.len()
        );
        return 1;
    }
    println!(
        "  PASS 边 {edge_ok} 条（节点引用全部合法），节点非零 uid {nonzero}/{}",
        node_count.min(100)
    );
    println!("  PASS kind 分布: {:?}", edge_kinds);
    println!("  PASS geometry 总点 {geom_point_count}（边几何累计 {geom_points_seen}）speed_limit/road_class 校验 {speed_sane}/{class_sane}");
    0
}

fn smoke_junction(path: &Path) -> i32 {
    println!("== junction.graph ==");
    let data = match read_all(path) {
        Ok(d) => d,
        Err(e) => {
            println!("  FAIL {e}");
            return 1;
        }
    };
    if data.len() < 19 {
        println!("  FAIL 文件过小 {}", data.len());
        return 1;
    }
    let magic = &data[0..7];
    if magic != JUNCTION_MAGIC {
        println!("  FAIL magic 不符");
        return 1;
    }
    let version = u32_le(&data[7..11]);
    let endianness = u32_le(&data[11..15]);
    let junc_count = u32_le(&data[15..19]) as usize;
    println!("  version={version} junctions={junc_count}");
    if version != EXPECTED_VERSION {
        println!("  FAIL version 不符（期望 {EXPECTED_VERSION}）");
        return 1;
    }
    if endianness != EXPECTED_ENDIANNESS {
        println!("  FAIL endianness 不符");
        return 1;
    }
    let mut pos = 19;
    let mut total_movements = 0;
    let mut total_geom_points: usize = 0;
    let mut max_geom_offset: usize = 0;
    let mut next_geom_offset: usize = 0;   // 期望的下一条 movement 几何 offset（坐标值索引）
    let mut tokens = std::collections::HashSet::new();
    for i in 0..junc_count {
        if pos + 8 + 64 + 1 + 4 > data.len() {
            println!("  FAIL junction {i} 头越界");
            return 1;
        }
        let uid = u64_le(&data[pos..pos + 8]);
        let token = String::from_utf8_lossy(&data[pos + 8..pos + 72])
            .trim_end_matches('\0')
            .to_string();
        tokens.insert(token.clone());
        if i < 2 {
            println!("  junction {i}: uid={uid:x} token={token}");
        }
        let node_count = data[pos + 72] as usize;
        pos += 73;
        if pos + node_count * 8 > data.len() {
            println!("  FAIL junction {i} 节点越界");
            return 1;
        }
        pos += node_count * 8;
        let movement_count = u32_le(&data[pos..pos + 4]) as usize;
        pos += 4;
        if i < 2 {
            println!("    nodes={node_count} movements={movement_count}");
        }
        for m in 0..movement_count {
            // v2 movement: id u32 + entry u64 + exit u64 + length f32 + turn i8
            //              + semaphore_id i32 + gt_len u8 + gt + geom_offset u32 + geom_count u16
            if pos + 36 > data.len() {
                println!("  FAIL junction {i} movement {m} 越界");
                return 1;
            }
            let id = u32_le(&data[pos..pos + 4]);
            let entry = u64_le(&data[pos + 4..pos + 12]);
            let exit = u64_le(&data[pos + 12..pos + 20]);
            if entry == exit {
                println!("  FAIL junction {i} movement {m} 自环");
                return 1;
            }
            if m == 0 && id != 0 {
                println!("  FAIL junction {i} movement 0 id={id}（应 0 起连续）");
                return 1;
            }
            let gt_len = data[pos + 29] as usize;
            // 布局：id(4) entry(8) exit(8) len(4) turn(1) sid(4) gt_len(1) gt(n) off(4) cnt(2)
            // off/cnt 在 gt 字符串之后（修正：Berlin gt 全空时与固定偏移重合，Germany 有类型字符串暴露错位）
            let geom_offset = u32_le(&data[pos + 30 + gt_len..pos + 34 + gt_len]) as usize;
            let geom_count = u16_le(&data[pos + 34 + gt_len..pos + 36 + gt_len]) as usize;
            if geom_offset % 3 != 0 || (geom_count != 0 && geom_count < 2) {
                println!("  FAIL junction {i} movement {m} 几何异常 off={geom_offset} cnt={geom_count}");
                return 1;
            }
            if geom_offset != next_geom_offset {
                println!("  FAIL junction {i} movement {m} 几何偏移不连续 off={geom_offset} 期望 {next_geom_offset}");
                return 1;
            }
            next_geom_offset += geom_count * 3;
            pos += 36 + gt_len;
            total_movements += 1;
            total_geom_points += geom_count;
            max_geom_offset = max_geom_offset.max(geom_offset);
        }
    }
    if pos + total_geom_points * 12 != data.len() {
        println!(
            "  FAIL 尾部偏移不符 pos={pos} geom_points={total_geom_points} len={}",
            data.len()
        );
        return 1;
    }
    println!(
        "  PASS junction {junc_count} 个（movements {total_movements}，无自环，显式 id 0 起），prefab 种数 {}",
        tokens.len()
    );
    println!("  PASS movement 几何总点 {total_geom_points}（末偏移 {max_geom_offset}）");
    0
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
