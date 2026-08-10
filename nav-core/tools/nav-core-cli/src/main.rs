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
        ("route", _) if args.len() >= 4 => route_cli(&args[2], &args[3]),
        ("route-verify", _) if args.len() >= 3 => route_verify(&args[2]),
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

/// route-verify：Europe 随机 OD 的 A*==Dijkstra 回归（§71）+ 长距离路线。
fn route_verify(dataset_dir: &str) {
    let (routing, _j) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let graph = nav_graph::CompactGraph::build(&routing);
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    let mut router = nav_router::search::Router::new(graph.node_count());
    // 长距离：Berlin 市区 → 北向（跨城）
    for (a, b, name) in [
        ((-58456.0, 32832.0), (-58456.0, 35000.0), "Berlin→北向"),
        ((-58456.0, 32832.0), (-60500.0, 32800.0), "Berlin→西向"),
    ] {
        let s1 = nav_router::snap::snap_nearest(&graph, &spatial, a.0, a.1, 300.0).unwrap();
        let s2 = nav_router::snap::snap_nearest(&graph, &spatial, b.0, b.1, 300.0).unwrap();
        let t0 = std::time::Instant::now();
        let req = nav_router::search::RouteRequest::new(
            &graph,
            nav_router::snap::VirtualEndpoint::start(&s1, true),
            nav_router::snap::VirtualEndpoint::goal(&s2),
            nav_router::cost::RouteProfile::Fastest,
        );
        match router.astar(&req) {
            Some(r) => println!(
                "[{name}] {:.0}m {:.0}s {} 边 {:.1}ms",
                r.distance_m,
                r.eta_s,
                r.edges.len(),
                t0.elapsed().as_secs_f64() * 1000.0
            ),
            None => println!("[{name}] 无路线"),
        }
    }
    // A*==Dijkstra 随机 OD（50 组 × 三 profile；LCG 伪随机固定种子）
    let mut ok = 0u32;
    let mut fail = 0u32;
    for i in 0..50 {
        // 伪随机：固定种子线性同余，落在 Berlin 附近核心网
        let seed = 20260810u64.wrapping_add(i as u64 * 2654435761);
        let mut s = seed;
        let mut rnd = move || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 33) as f64 / (1u64 << 31) as f64
        };
        let xa = -62000.0 + rnd() * 9000.0;
        let za = 30000.0 + rnd() * 8000.0;
        let xb = -62000.0 + rnd() * 9000.0;
        let zb = 30000.0 + rnd() * 8000.0;
        let Some(s1) = nav_router::snap::snap_nearest(&graph, &spatial, xa, za, 300.0) else {
            continue;
        };
        let Some(s2) = nav_router::snap::snap_nearest(&graph, &spatial, xb, zb, 300.0) else {
            continue;
        };
        for profile in [
            nav_router::cost::RouteProfile::Shortest,
            nav_router::cost::RouteProfile::Fastest,
            nav_router::cost::RouteProfile::Balanced,
        ] {
            let req = nav_router::search::RouteRequest::new(
                &graph,
                nav_router::snap::VirtualEndpoint::start(&s1, true),
                nav_router::snap::VirtualEndpoint::goal(&s2),
                profile,
            );
            let d = router.dijkstra(&req);
            let a = router.astar(&req);
            match (d, a) {
                (Some(dr), Some(ar)) => {
                    let cd = nav_router::search::route_cost(&graph, &dr, profile);
                    let ca = nav_router::search::route_cost(&graph, &ar, profile);
                    if (cd - ca).abs() < 1e-3 {
                        ok += 1;
                    } else {
                        fail += 1;
                        if fail <= 3 {
                            println!("  不一致 {profile:?}: D={cd:.3} A*={ca:.3}");
                        }
                    }
                }
                (None, None) => ok += 1,
                _ => {
                    fail += 1;
                    println!("  可达性不一致");
                }
            }
        }
    }
    println!("A*==Dijkstra 回归: {ok} 一致 / {fail} 不一致");
    // —— RouteTracker 模拟：沿 Berlin 路线行驶，progress 单调 ——
    let s1 = nav_router::snap::snap_nearest(&graph, &spatial, -58456.0, 32832.0, 300.0).unwrap();
    let s2 = nav_router::snap::snap_nearest(&graph, &spatial, -58456.0, 35000.0, 300.0).unwrap();
    let req = nav_router::search::RouteRequest::new(
        &graph,
        nav_router::snap::VirtualEndpoint::start(&s1, true),
        nav_router::snap::VirtualEndpoint::goal(&s2),
        nav_router::cost::RouteProfile::Fastest,
    );
    if let Some(route) = router.astar(&req) {
        let mut tracker = nav_router::tracker::RouteTracker::new(&graph, route.clone(), 4);
        // 沿每条边的几何点逐步推进
        let mut last_p = -1.0;
        let mut mono = true;
        for (i, &eid) in route.edges.iter().enumerate() {
            let e = &graph.edges[eid as usize];
            let pts = graph.edge_geometry(e);
            for (k, (x, _, z)) in pts.iter().enumerate() {
                // 用几何点位置反查 matcher 无必要——直接按边序/弧长推进
                let off = nav_graph::project_point(pts, *x, *z).1;
                let _ = off;
                let seg_len = nav_graph::polyline_length(&pts[..=k.min(pts.len() - 1)]);
                let u = tracker.update(&graph, eid, seg_len);
                if u.distance_travelled < last_p - 1e-6 {
                    mono = false;
                }
                last_p = u.distance_travelled;
            }
            if i % 10 == 0 {
                let u = tracker.update(&graph, eid, 0.0);
                println!(
                    "  tracker@{i}: matched={} travelled={:.0}m remaining={:.0}m progress={:.2}",
                    u.matched,
                    u.distance_travelled,
                    u.remaining_distance,
                    tracker.progress()
                );
            }
        }
        let final_p = tracker.progress();
        println!(
            "tracker 模拟：{} 边走完，progress={:.3} 单调={}，总长 {:.0}m",
            route.edges.len(),
            final_p,
            mono,
            route.distance_m
        );
        assert!(final_p > 0.9, "走完路线后 progress 应接近 1: {final_p}");
        // —— Reroute 模拟：沿路线走 40 边后强制偏航 12 帧 → 检测器 OFF_ROUTE → 重规划 ——
        let mut det = nav_router::reroute::RerouteDetector::new(
            nav_router::reroute::RerouteConfig::default(),
        );
        let mut states = Vec::new();
        for i in 0..60 {
            // 前 40 帧匹配（沿路线），后 20 帧窗口外（偏航，20m/帧）
            let matched = i < 40;
            states.push(det.on_frame(matched, 20.0));
        }
        println!(
            "reroute 模拟：状态序列 {:?}（应含 ON_ROUTE→SUSPECTED→OFF_ROUTE）",
            states
        );
        assert!(states.contains(&nav_router::reroute::OffRouteState::OffRoute));
        // 重规划：当前 snap（偏航点）+ 同 destination → 新路线（§91）
        det.begin_rerouting();
        let off_snap =
            nav_router::snap::snap_nearest(&graph, &spatial, -58200.0, 33600.0, 300.0).unwrap();
        let t0 = std::time::Instant::now();
        let new_route = nav_router::reroute::reroute(
            &graph,
            &mut router,
            &off_snap,
            &s2,
            nav_router::cost::RouteProfile::Fastest,
        );
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        match new_route {
            Some(r) => println!(
                "重规划成功：{:.0}m {:.0}s {} 边 {:.1}ms（§93 目标 ≈1s）",
                r.distance_m,
                r.eta_s,
                r.edges.len(),
                ms
            ),
            None => println!("重规划失败"),
        }
        det.reset();
        println!("检测器已重置: {}", det.state().name());
    }
}

/// route：A* 路线规划（§124：distance/ETA/signals/edges/maneuvers；三 profile 对比）。
fn route_cli(xz: &str, dataset_dir: &str) {
    let parts: Vec<&str> = xz.split(':').collect();
    if parts.len() != 2 {
        eprintln!("格式: <x1,z1:x2,z2> 如 -58456,32832:-52925,36510");
        std::process::exit(1);
    }
    let parse = |s: &str| -> (f64, f64) {
        let v: Vec<&str> = s.split(',').collect();
        (v[0].trim().parse().unwrap(), v[1].trim().parse().unwrap())
    };
    let (x1, z1) = parse(parts[0]);
    let (x2, z2) = parse(parts[1]);
    let (routing, _j) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let graph = nav_graph::CompactGraph::build(&routing);
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    let s1 = nav_router::snap::snap_nearest(&graph, &spatial, x1, z1, 300.0).unwrap_or_else(|| {
        eprintln!("起点 300m 内无可路由边");
        std::process::exit(1);
    });
    let s2 = nav_router::snap::snap_nearest(&graph, &spatial, x2, z2, 300.0).unwrap_or_else(|| {
        eprintln!("终点 300m 内无可路由边");
        std::process::exit(1);
    });
    let mut router = nav_router::search::Router::new(graph.node_count());
    println!(
        "起 ({x1:.0},{z1:.0}) 吸附 {:.0}m → 终 ({x2:.0},{z2:.0}) 吸附 {:.0}m",
        s1.lateral, s2.lateral
    );
    // 备选路线（§79-82）
    let alts = nav_router::alternatives::plan_alternatives(
        &graph,
        &mut router,
        nav_router::snap::VirtualEndpoint::start(&s1, true),
        nav_router::snap::VirtualEndpoint::goal(&s2),
        &nav_router::alternatives::AltParams::default(),
    );
    println!("备选路线 {} 条（overlap 去重后）", alts.routes.len());
    for (i, r) in alts.routes.iter().enumerate() {
        println!(
            "  #{i} [{}] {:.0}m {:.0}s（{:.0} min）{} 边",
            alts.profiles[i].name(),
            r.distance_m,
            r.eta_s,
            r.eta_s / 60.0,
            r.edges.len()
        );
    }
    for profile in [
        nav_router::cost::RouteProfile::Fastest,
        nav_router::cost::RouteProfile::Shortest,
        nav_router::cost::RouteProfile::Balanced,
    ] {
        let t0 = std::time::Instant::now();
        let start = nav_router::snap::VirtualEndpoint::start(&s1, true);
        let goal = nav_router::snap::VirtualEndpoint::goal(&s2);
        let req = nav_router::search::RouteRequest::new(&graph, start, goal, profile);
        match router.astar(&req) {
            Some(r) => {
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                println!("[{}] {:>6.0}m {:>7.0}s（{:.0} min）{} 边（road {} / jct {} / sig {} / ferry {} / train {}）{:.1} ms",
                    profile.name(), r.distance_m, r.eta_s, r.eta_s / 60.0, r.edges.len(),
                    r.road_edge_count, r.junction_count, r.signal_count, r.ferry_count, r.train_count, ms);
            }
            None => println!("[{}] 无路线", profile.name()),
        }
    }
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
