// nav-core-cli：Navigation Core 开发命令行（P2 计划 §122-127）。
// 当前实现 dataset info（加载时间/内存/统计）；route/match/replay/live/bench 后续工作包加入。
use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: nav-core-cli <dataset|info> <dataset-dir>");
        eprintln!("  nav-core-cli dataset info <dir>   —— 显示 dataset 统计与加载基准");
        std::process::exit(2);
    }
    match (args[1].as_str(), args.get(2).map(|s| s.as_str())) {
        ("dataset", Some("info")) if args.len() >= 4 => dataset_info(&args[3]),
        ("info", _) if args.len() >= 3 => dataset_info(&args[2]),
        ("live", Some(path)) => live(Some(path)),
        ("live", _) => live(None),
        ("replay", _) if args.len() >= 3 => replay_trace(&args[2]),
        ("match", _) if args.len() >= 4 => match_trace(&args[2], &args[3]),
        ("snap", _) if args.len() >= 4 => snap_cli(&args[2], &args[3]),
        _ => {
            eprintln!("用法:");
            eprintln!("  nav-core-cli dataset info <dataset-dir>");
            eprintln!("  nav-core-cli live [trace.navtrace]        —— 实时遥测（可选同时录制）");
            eprintln!("  nav-core-cli replay <trace.navtrace>      —— 回放 trace");
            eprintln!("  nav-core-cli match <trace> <dataset-dir>   —— trace 回放 Map Matching");
            eprintln!(
                "  nav-core-cli snap <x,z> <dataset-dir>       —— 目的地吸附（最近可路由 edge）"
            );
            std::process::exit(2);
        }
    }
}

/// 实时遥测：连接共享内存，持续输出帧摘要；可选录制 trace。
fn live(trace_path: Option<&str>) {
    let mut src = nav_telemetry::TelemetrySource::new(std::time::Duration::from_secs(2));
    let mut rec = trace_path
        .map(|p| nav_telemetry::TraceRecorder::create(std::path::Path::new(p)))
        .transpose()
        .unwrap_or_else(|e| {
            eprintln!("无法创建 trace: {e}");
            std::process::exit(1);
        });
    let mut det = nav_telemetry::EventDetector::new(50.0);
    let mut last_shown = std::time::Instant::now();
    println!("等待遥测桥（Local\\ETS2NavTelemetry）……");
    loop {
        match src.poll() {
            nav_telemetry::TelemetryState::Fresh(snap) => {
                if let Some(r) = rec.as_mut() {
                    let _ = r.record(&snap);
                }
                for ev in det.feed(&snap) {
                    println!("EVENT: {ev:?}");
                }
                if last_shown.elapsed().as_millis() >= 500 {
                    println!(
                        "TELEMETRY seq={} sim={:.1}s pos=({:.1},{:.1},{:.1}) speed={:.1} m/s limit={:.1} paused={} job={}",
                        snap.sequence,
                        snap.simulation_time as f64 / 1e6,
                        snap.position[0], snap.position[1], snap.position[2],
                        snap.speed, snap.speed_limit, snap.paused,
                        snap.job.as_ref().map(|j| j.dest_city.as_str()).unwrap_or("-"),
                    );
                    last_shown = std::time::Instant::now();
                }
            }
            nav_telemetry::TelemetryState::Stale => {
                // 静默（避免刷屏）；状态不推进由上层处理
            }
            nav_telemetry::TelemetryState::Disconnected => {
                std::thread::sleep(std::time::Duration::from_millis(500));
                src.reconnect();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(16)); // ~60 Hz 轮询上限
    }
}

/// 回放 trace：逐帧打印摘要（后续工作包接入 matcher/reroute 后扩展）。
fn replay_trace(path: &str) {
    let frames: Vec<nav_telemetry::TraceFrame> = nav_telemetry::replay(std::path::Path::new(path))
        .unwrap_or_else(|e| {
            eprintln!("打开 trace 失败: {e}");
            std::process::exit(1);
        })
        .collect();
    if frames.is_empty() {
        eprintln!("trace 为空或无法解析");
        std::process::exit(1);
    }
    let dur = frames.last().unwrap().t - frames.first().unwrap().t;
    println!(
        "trace: {} 帧，时长 {:.1}s，起点 ({:.1},{:.1},{:.1})，终点 ({:.1},{:.1},{:.1})",
        frames.len(),
        dur,
        frames.first().unwrap().snap.position[0],
        frames.first().unwrap().snap.position[1],
        frames.first().unwrap().snap.position[2],
        frames.last().unwrap().snap.position[0],
        frames.last().unwrap().snap.position[1],
        frames.last().unwrap().snap.position[2],
    );
    // 事件检测回放
    let mut det = nav_telemetry::EventDetector::new(50.0);
    let mut ev_count = 0;
    for f in &frames {
        for ev in det.feed(&f.snap) {
            println!("  t={:.1}s EVENT: {ev:?}", f.t);
            ev_count += 1;
        }
    }
    println!("事件总数: {ev_count}");
    println!("PASS");
}

/// trace 回放 Map Matching：逐帧匹配并统计置信度/横向距离（P2-06 验证）。
fn match_trace(trace_path: &str, dataset_dir: &str) {
    let (routing, _j) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let graph = nav_graph::CompactGraph::build(&routing);
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    let frames: Vec<nav_telemetry::TraceFrame> =
        nav_telemetry::replay(std::path::Path::new(trace_path))
            .unwrap_or_else(|e| {
                eprintln!("打开 trace 失败: {e}");
                std::process::exit(1);
            })
            .collect();
    if frames.is_empty() {
        eprintln!("trace 为空");
        std::process::exit(1);
    }
    let mut m = nav_matcher::MapMatcher::new(nav_matcher::MatcherConfig::default());
    let (mut hi, mut med, mut low, mut un) = (0u32, 0u32, 0u32, 0u32);
    let mut lateral_sum = 0.0f64;
    let mut matched = 0u32;
    let mut edge_hist = std::collections::HashMap::<u32, u32>::new();
    for f in &frames {
        let p = f.snap.position;
        let yaw = quat_yaw(f.snap.heading);
        let mm = m.match_frame(&graph, &spatial, p[0], p[2], yaw);
        match mm.confidence {
            nav_matcher::MatchConfidence::High => {
                hi += 1;
                lateral_sum += mm.lateral;
                matched += 1;
            }
            nav_matcher::MatchConfidence::Medium => {
                med += 1;
                lateral_sum += mm.lateral;
                matched += 1;
            }
            nav_matcher::MatchConfidence::Low => low += 1,
            nav_matcher::MatchConfidence::Unmatched => un += 1,
        }
        if mm.edge_id != u32::MAX {
            *edge_hist.entry(mm.edge_id).or_default() += 1;
        }
    }
    let n = frames.len();
    println!(
        "trace: {n} 帧，起点 ({:.0},{:.0})，终点 ({:.0},{:.0})",
        frames.first().unwrap().snap.position[0],
        frames.first().unwrap().snap.position[2],
        frames.last().unwrap().snap.position[0],
        frames.last().unwrap().snap.position[2]
    );
    println!("匹配: HIGH {hi} / MEDIUM {med} / LOW {low} / UNMATCHED {un}");
    if matched > 0 {
        println!("匹配帧横向距离均值: {:.1} m", lateral_sum / matched as f64);
    }
    // 主边占比（锁定稳定性）
    let mut top: Vec<(u32, u32)> = edge_hist.into_iter().collect();
    top.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    println!("锁定边 TOP5: {:?}", top.iter().take(5).collect::<Vec<_>>());
}

/// snap：任意坐标 → 最近可路由 edge（P2-07 §56）。
fn snap_cli(xz: &str, dataset_dir: &str) {
    let parts: Vec<&str> = xz.split(',').collect();
    if parts.len() != 2 {
        eprintln!("格式: <x,z> 如 -58456,32832");
        std::process::exit(1);
    }
    let x: f64 = parts[0].trim().parse().unwrap_or_else(|_| {
        eprintln!("x 解析失败");
        std::process::exit(1);
    });
    let z: f64 = parts[1].trim().parse().unwrap_or_else(|_| {
        eprintln!("z 解析失败");
        std::process::exit(1);
    });
    let (routing, _j) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let graph = nav_graph::CompactGraph::build(&routing);
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    let hits = spatial.query_radius(x, z, 300.0);
    println!("候选边数: {}", hits.len());
    let s = nav_router::snap::snap_nearest(&graph, &spatial, x, z, 300.0).unwrap_or_else(|| {
        eprintln!("300m 内无可路由边");
        std::process::exit(1);
    });
    let e = &graph.edges[s.edge_id as usize];
    println!(
        "snap: edge={} kind={:?} offset={:.1}m lateral={:.1}m pos=({:.1},{:.1})",
        s.edge_id, e.kind, s.offset, s.lateral, s.position.0, s.position.2
    );
    println!(
        "edge 总长: {:.1}m（forward 剩余 {:.1}m / backward {:.1}m）",
        s.edge_length(&graph),
        s.edge_length(&graph) - s.offset,
        s.offset
    );
}

/// 四元数 → yaw（世界弧度；SCS quat (x,y,z,w)）。
fn quat_yaw(q: [f32; 4]) -> f64 {
    let (x, y, z, w) = (q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64);
    (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z))
}

fn dataset_info(dir: &str) {
    let t0 = Instant::now();
    match nav_dataset::load_dataset(Path::new(dir)) {
        Ok((routing, junction)) => {
            let load = t0.elapsed();
            let st = nav_graph::stats(&routing);
            println!("dataset: {dir}");
            println!("  version: v{}", nav_dataset::EXPECTED_DATASET_VERSION);
            println!(
                "  加载: {:.2} s（{} nodes / {} edges / {} junctions / {} movements）",
                load.as_secs_f64(),
                routing.nodes.len(),
                routing.edges.len(),
                junction.junctions.len(),
                junction
                    .junctions
                    .iter()
                    .map(|j| j.movements.len())
                    .sum::<usize>()
            );
            println!(
                "  CSR 压缩: {} raw → {} active nodes（{:.1}%）",
                st.raw_nodes,
                st.active_nodes,
                100.0 * st.active_nodes as f64 / st.raw_nodes.max(1) as f64
            );
            println!(
                "  边: Road {} / Movement {} / Transit {} / 总 {}",
                st.road_edges, st.movement_edges, st.transit_edges, st.edges
            );
            println!("  几何点: {}（routing.graph）", st.geometry_points);
            println!("  speed_limit 未知(-1): {} 边", st.unknown_speed_edges);
            // 粗略内存估计（边 + 几何）
            let mem = st.edges * 40 + st.geometry_points * 24;
            println!("  粗略内存（边+几何）: {:.1} MB", mem as f64 / 1e6);
            // 校验：边连续性抽样 + spatial 索引基准
            let c = nav_graph::CompactGraph::build(&routing);
            let t1 = std::time::Instant::now();
            let sp = nav_spatial::SpatialIndex::build(&c, nav_spatial::DEFAULT_CELL_SIZE);
            let (n_bbox, n_cell) = sp.stats();
            let build_ms = t1.elapsed().as_secs_f64() * 1000.0;
            // 查询基准：Berlin 中心附近 500 点
            let t2 = std::time::Instant::now();
            let mut hits = 0usize;
            for i in 0..500 {
                let x = -58456.0 + (i as f64 % 50.0) * 40.0;
                let z = 32832.0 + (i as f64 / 50.0) * 40.0;
                hits += sp.query_radius(x, z, 100.0).len();
            }
            let query_ms = t2.elapsed().as_secs_f64() * 1000.0;
            println!(
                "  spatial: {} 边索引 / {} cells，构建 {:.1} ms，500 查询 {:.1} ms（{} 候选）",
                n_bbox, n_cell, build_ms, query_ms, hits
            );
            let mut edge_ok = 0;
            for e in &c.edges {
                if e.from < c.node_count() as u32 && e.to < c.node_count() as u32 {
                    edge_ok += 1;
                }
            }
            println!("  CSR 校验: {edge_ok}/{} 边端点合法", c.edges.len());
            // movement 与 junction 关联抽样：第一个带灯 movement
            let mut lit = 0;
            for j in &junction.junctions {
                for m in &j.movements {
                    if m.semaphore_id >= 0 {
                        lit += 1;
                    }
                }
            }
            println!("  带灯 movement: {lit}");
            println!("  PASS");
        }
        Err(e) => {
            eprintln!("加载失败: {e}");
            std::process::exit(1);
        }
    }
}
