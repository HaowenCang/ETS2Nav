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
        _ => {
            eprintln!("用法:");
            eprintln!("  nav-core-cli dataset info <dataset-dir>");
            eprintln!("  nav-core-cli live [trace.navtrace]        —— 实时遥测（可选同时录制）");
            eprintln!("  nav-core-cli replay <trace.navtrace>      —— 回放 trace");
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
        std::thread::sleep(std::time::Duration::from_millis(16));   // ~60 Hz 轮询上限
    }
}

/// 回放 trace：逐帧打印摘要（后续工作包接入 matcher/reroute 后扩展）。
fn replay_trace(path: &str) {
    let frames: Vec<nav_telemetry::TraceFrame> =
        nav_telemetry::replay(std::path::Path::new(path))
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
            // 校验：边连续性抽样
            let c = nav_graph::CompactGraph::build(&routing);
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
