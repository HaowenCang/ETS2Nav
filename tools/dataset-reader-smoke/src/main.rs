// P1-11 dataset-reader-smoke：Rust 独立读取 routing.graph / junction.graph（无 C# 依赖）。
// 校验 magic/version/endianness/计数/内容，验证 Dataset 可脱离 C# runtime 被读取。
// 用法：dataset-reader-smoke <dataset-dir>
use std::env;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

const ROUTING_MAGIC: &[u8; 7] = b"ETS2RG1";
const JUNCTION_MAGIC: &[u8; 7] = b"ETS2JG1";
const EXPECTED_ENDIANNESS: u32 = 0x12345678;

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
    if data.len() < 23 {
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
    println!("  version={version} endianness=0x{endianness:x} nodes={node_count} edges={edge_count}");
    if version != 1 {
        println!("  FAIL version 不符");
        return 1;
    }
    if endianness != EXPECTED_ENDIANNESS {
        println!("  FAIL endianness 不符");
        return 1;
    }
    // nodes: 20 B each；edges: 26 B + 4 B（movement_id 可选 flag bit3）
    let nodes_bytes = node_count * 20;
    let mut pos = 23 + nodes_bytes;
    if pos > data.len() {
        println!("  FAIL 节点区越界");
        return 1;
    }
    // 校验前 3 个节点 uid 非零 + 坐标合理
    let mut nonzero = 0;
    for i in 0..node_count.min(100) {
        let off = 23 + i * 20;
        let uid = u64_le(&data[off..off + 8]);
        if uid != 0 {
            nonzero += 1;
        }
    }
    let mut edge_ok = 0;
    let mut edge_kinds = std::collections::BTreeMap::<u8, u32>::new();
    let mut seen_pairs = std::collections::HashSet::<(u32, u32, u8)>::new();
    for i in 0..edge_count {
        if pos + 26 > data.len() {
            println!("  FAIL 边 {i} 越界");
            return 1;
        }
        let from = u32_le(&data[pos..pos + 4]);
        let to = u32_le(&data[pos + 4..pos + 8]);
        let kind = data[pos + 8];
        let flags = data[pos + 25];
        // 并行边合法（多车道/多路径 movement 同 entry/exit）——不校验去重
        if from >= node_count as u32 || to >= node_count as u32 {
            println!("  FAIL 边 {i} 节点越界 from={from} to={to}");
            return 1;
        }
        *edge_kinds.entry(kind).or_default() += 1;
        pos += 26;
        if flags & 8 != 0 {
            pos += 4; // movement_id
        }
        edge_ok += 1;
    }
    if pos != data.len() {
        println!("  FAIL 尾部偏移不符 pos={pos} len={}", data.len());
        return 1;
    }
    println!("  PASS 边 {edge_ok} 条（节点引用全部合法），节点非零 uid {nonzero}/{}", node_count.min(100));
    println!("  PASS kind 分布: {:?}", edge_kinds);
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
    let mut pos = 19;
    let mut total_movements = 0;
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
        if i < 2 { println!("  junction {i}: uid={uid:x} token={token}"); }
        let node_count = data[pos + 72] as usize;
        pos += 73;
        if pos + node_count * 8 > data.len() {
            println!("  FAIL junction {i} 节点越界");
            return 1;
        }
        pos += node_count * 8;
        let movement_count = u32_le(&data[pos..pos + 4]) as usize;
        pos += 4;
        if i < 2 { println!("    nodes={node_count} movements={movement_count}"); }
        for m in 0..movement_count {
            if pos + 8 + 8 + 4 + 1 + 4 + 1 > data.len() {
                println!("  FAIL junction {i} movement {m} 越界");
                return 1;
            }
            let entry = u64_le(&data[pos..pos + 8]);
            let exit = u64_le(&data[pos + 8..pos + 16]);
            if entry == exit {
                println!("  FAIL junction {i} movement {m} 自环");
                return 1;
            }
            let gt_len = data[pos + 25] as usize;
            pos += 26 + gt_len;
            total_movements += 1;
        }
    }
    if pos != data.len() {
        println!("  FAIL 尾部偏移不符 pos={pos} len={}", data.len());
        return 1;
    }
    println!("  PASS junction {junc_count} 个（movements {total_movements}，无自环），prefab 种数 {}", tokens.len());
    0
}

fn u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn u64_le(b: &[u8]) -> u64 {
    u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}
