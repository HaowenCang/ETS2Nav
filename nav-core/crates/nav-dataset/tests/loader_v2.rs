// nav-dataset loader v2 单元测试：手工构造最小 v2 二进制验证全链路解析。
use nav_dataset::*;
use std::io::Write;
use std::path::PathBuf;

fn tmp_file(test: &str, name: &str, bytes: &[u8]) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "navds_test_{}_{}_{}",
        std::process::id(),
        test,
        name
    ));
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(bytes).unwrap();
    p
}

fn u32(v: u32) -> [u8; 4] {
    v.to_le_bytes()
}
fn u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}
fn u64(v: u64) -> [u8; 8] {
    v.to_le_bytes()
}
fn i32f(v: i32) -> [u8; 4] {
    v.to_le_bytes()
}

/// routing.graph v2：3 节点 3 边（Road 双向 2 边 + Movement 1 边带灯带几何）。
fn routing_bytes() -> Vec<u8> {
    let mut b = b"ETS2RG1".to_vec();
    b.extend_from_slice(&u32(2)); // version
    b.extend_from_slice(&u32(0x12345678)); // endianness
    b.extend_from_slice(&u32(3)); // node_count
    b.extend_from_slice(&u32(3)); // edge_count
                                  // 几何点：e0=2点 e1=2点 e2=3点 → 7 点
    b.extend_from_slice(&u32(7)); // geom_point_count
                                  // nodes（20B）：(0,0,0) (100,0,0) (100,0,100)
    for (uid, x, z) in [
        (1u64, 0i32, 0i32),
        (2, 100 * 256, 0),
        (3, 100 * 256, 100 * 256),
    ] {
        b.extend_from_slice(&u64(uid));
        b.extend_from_slice(&i32f(x));
        b.extend_from_slice(&i32f(0));
        b.extend_from_slice(&i32f(z));
    }
    // e0: Road 0→1, geom 0..2, speed 50, class 1
    b.extend_from_slice(&u32(0));
    b.extend_from_slice(&u32(1));
    b.push(0);
    b.extend_from_slice(&100f32.to_le_bytes());
    b.extend_from_slice(&u64(100));
    b.extend_from_slice(&u32(0));
    b.extend_from_slice(&(2u16).to_le_bytes());
    b.extend_from_slice(&50i16.to_le_bytes());
    b.push(1);
    b.extend_from_slice(&(-1i32).to_le_bytes());
    b.push(0);
    // e1: Road 1→0（反向）
    b.extend_from_slice(&u32(1));
    b.extend_from_slice(&u32(0));
    b.push(0);
    b.extend_from_slice(&100f32.to_le_bytes());
    b.extend_from_slice(&u64(100));
    b.extend_from_slice(&u32(6));
    b.extend_from_slice(&(2u16).to_le_bytes());
    b.extend_from_slice(&50i16.to_le_bytes());
    b.push(1);
    b.extend_from_slice(&(-1i32).to_le_bytes());
    b.push(0);
    // e2: Movement 1→2, 带灯 sid=0, movement_id=7, 几何 3 点（12..15）
    b.extend_from_slice(&u32(1));
    b.extend_from_slice(&u32(2));
    b.push(1);
    b.extend_from_slice(&30f32.to_le_bytes());
    b.extend_from_slice(&u64(500));
    b.extend_from_slice(&u32(12));
    b.extend_from_slice(&(3u16).to_le_bytes());
    b.extend_from_slice(&30i16.to_le_bytes());
    b.push(2);
    b.extend_from_slice(&0i32.to_le_bytes());
    b.push(8); // flags bit3 = 有 movement_id
    b.extend_from_slice(&u32(7)); // movement_id
                                  // geometry 7 点（1/256 定点）
    for (x, y, z) in [
        (0i32, 0i32, 0i32),
        (100 * 256, 0, 0),
        (100 * 256, 0, 0),
        (100 * 256, 0, 100 * 256),
        (100 * 256, 0, 0),
        (100 * 256, 0, 50 * 256),
        (100 * 256, 0, 100 * 256),
    ] {
        b.extend_from_slice(&i32f(x));
        b.extend_from_slice(&i32f(y));
        b.extend_from_slice(&i32f(z));
    }
    b
}

#[test]
fn routing_v2_full_parse() {
    let p = tmp_file("full_parse", "routing.graph", &routing_bytes());
    let g = load_routing_graph(&p).expect("解析成功");
    assert_eq!(g.nodes.len(), 3);
    assert_eq!(g.edges.len(), 3);
    // e0 Road
    let e0 = &g.edges[0];
    assert_eq!(e0.kind, EdgeKind::Road);
    assert_eq!(e0.from, 0);
    assert_eq!(e0.to, 1);
    assert_eq!(e0.speed_limit, 50);
    assert_eq!(e0.road_class, 1);
    assert_eq!(e0.semaphore_id, -1);
    assert_eq!(e0.geometry.len(), 2);
    assert!((e0.geometry[0].0 - 0.0).abs() < 1e-6);
    // e2 Movement：带灯 + movement_id + 3 点几何
    let e2 = &g.edges[2];
    assert_eq!(e2.kind, EdgeKind::JunctionMovement);
    assert_eq!(e2.semaphore_id, 0);
    assert_eq!(e2.movement_id, Some(7));
    assert_eq!(e2.geometry.len(), 3);
    assert!((e2.geometry[1].2 - 50.0).abs() < 1e-6);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn routing_v2_version_mismatch_rejected() {
    let mut b = routing_bytes();
    b[7..11].copy_from_slice(&u32(1)); // version 1 → 拒绝
    let p = tmp_file("ver_mismatch", "routing.graph", &b);
    let err = load_routing_graph(&p).unwrap_err();
    match err {
        DatasetError::VersionMismatch {
            found: 1,
            expected: 2,
        } => {}
        other => panic!("期望 VersionMismatch，得到 {other}"),
    }
    let _ = std::fs::remove_file(&p);
}

#[test]
fn routing_v2_corrupt_rejected() {
    let mut b = routing_bytes();
    b.truncate(b.len() - 5); // 截断 → 尾部偏移不符
    let p = tmp_file("corrupt", "routing.graph", &b);
    assert!(matches!(
        load_routing_graph(&p),
        Err(DatasetError::Corrupt(_))
    ));
    let _ = std::fs::remove_file(&p);
}
