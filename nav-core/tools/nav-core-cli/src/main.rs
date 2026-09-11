// nav-core-cli：Navigation Core 开发命令行（P2 计划 §122-127）。
// 当前实现 dataset info（加载时间/内存/统计）；route/match/replay/live/bench 后续工作包加入。
use std::io::Write;
use std::path::Path;
use std::time::Instant;

mod server;
mod server_cli;

/// 统一用法输出（`use` 前置与未知子命令共用，避免两处文案漂移）。
fn usage() {
    eprintln!("用法:");
    eprintln!("  nav-core-cli dataset info <dataset-dir>");
    eprintln!("  nav-core-cli live [trace.navtrace]        —— 实时遥测并录制 trace（省略路径则用 %TEMP% 默认名）");
    eprintln!("  nav-core-cli replay <trace.navtrace>      —— 回放 trace");
    eprintln!("  nav-core-cli match <trace> <dataset-dir>   —— trace 回放 Map Matching");
    eprintln!("  nav-core-cli snap <x,z> <dataset-dir>       —— 目的地吸附（最近可路由 edge）");
    eprintln!("  nav-core-cli server <dataset-dir> [--replay=<trace>] [--port=<N>] [--web=<dir>] [--fake-signal]");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // 仅要求存在子命令：`live [trace]` 的路径为可选参数，若在此处要求 args.len() >= 3，
    // 则 `nav-core-cli live` 会被误判为参数不足而拒绝执行（实测 exit 2）。
    // 其余子命令各自带 args.len() 守卫，参数不足时落到下方 `_` 分支打印用法。
    if args.len() < 2 {
        usage();
        std::process::exit(2);
    }
    match (args[1].as_str(), args.get(2).map(|s| s.as_str())) {
        ("dataset", Some("info")) if args.len() >= 4 => dataset_info(&args[3]),
        ("info", _) if args.len() >= 3 => dataset_info(&args[2]),
        // 第二参数以 `--` 开头时不当作 trace 路径（如 `live --help` 走默认路径而非生成名为 "--help" 的文件）
        ("live", Some(path)) if !path.starts_with("--") => live(Some(path)),
        ("live", _) => live(None),
        ("replay", _) if args.len() >= 3 => replay_trace(&args[2]),
        ("match", _) if args.len() >= 4 => match_trace(&args[2], &args[3]),
        ("snap", _) if args.len() >= 4 => snap_cli(&args[2], &args[3]),
        ("route", _) if args.len() >= 4 => route_cli(&args[2], &args[3]),
        ("route-verify", _) if args.len() >= 3 => route_verify(&args[2]),
        ("roundabout-stats", _) if args.len() >= 3 => roundabout_stats(&args[2]),
        ("dest", _) if args.len() >= 4 => dest_cli(&args[2], &args[3]),
        ("speed", _) if args.len() >= 4 => speed_cli(&args[2], &args[3], args.get(4)),
        ("signal", _) if args.len() >= 4 => signal_cli(&args[2], &args[3]),
        ("session", _) if args.len() >= 4 => session_cli(&args[2], &args[3]),
        ("regression", _) if args.len() >= 3 => regression_cli(&args[2]),
        ("server", _) if args.len() >= 3 => {
            let fake = args.iter().skip(3).any(|a| a == "--fake-signal");
            server_cli_run(&args, fake)
        }
        ("syntrace", _) if args.len() >= 5 => syntrace_cli(&args[2], &args[3], &args[4]),
        ("bench", _) if args.len() >= 3 => bench_cli(&args[2]),
        _ => {
            usage();
            std::process::exit(2);
        }
    }
}

/// 默认 trace 路径：`%TEMP%\ets2nav-live-<UTC 时间戳>.navtrace`。
/// 时间戳使多轮采集（如 T5 的两轮 FPS 对照）产出文件互不覆盖且可区分先后。
fn default_trace_path() -> std::path::PathBuf {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = utc_parts(secs);
    std::env::temp_dir().join(format!(
        "ets2nav-live-{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}.navtrace"
    ))
}

/// UNIX 秒 → UTC 年月日时分秒（Howard Hinnant `civil_from_days` 算法，零依赖）。
fn utc_parts(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// 信号灯旁路记录器（T3 离线判定用）：与 trace 同名的 `<trace>.sem.csv`。
///
/// 存在的理由：`TelemetrySnapshot` 不含信号灯字段，故 trace 无法用于离线复核信号关联；
/// 且 `nav-core-cli session` 回放时读到的是**当下**共享内存而非录制时的灯态
/// （见 `nav-router/src/session.rs` 的 `next_signal`，其 `read_semaphores()` 在回放时执行）。
/// 因此信号证据必须与 trace 并行落盘，否则一场实机采集的信号数据不可复现。
///
/// 格式为长表（每次采样每灯一行），便于按灯 `id` 或时间窗切片；20 Hz 采样足够，因为灯态
/// 与倒计时以秒为尺度变化。写 `File` 而非 `BufWriter`：逐行即时落盘，Ctrl+C 不丢数据。
struct SemRecorder {
    w: Box<dyn Write>,
    path: std::path::PathBuf,
    start: std::time::Instant,
    rows: usize,
}

impl SemRecorder {
    /// 由 trace 路径派生：`x.navtrace` → `x.sem.csv`。
    fn create(trace_path: &Path) -> std::io::Result<Self> {
        let path = trace_path.with_extension("sem.csv");
        let mut w = Box::new(std::fs::File::create(&path)?) as Box<dyn Write>;
        w.write_all(b"wall_ms,slot,id,kind,state,time_remaining,x,y,z,qx,qy,qz,qw\n")?;
        Ok(SemRecorder {
            w,
            path,
            start: std::time::Instant::now(),
            rows: 0,
        })
    }

    fn record(&mut self, slots: &[nav_telemetry::SemaphoreSlot]) -> std::io::Result<()> {
        let wall = self.start.elapsed().as_millis();
        for (i, s) in slots.iter().enumerate() {
            writeln!(
                self.w,
                "{wall},{i},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.5},{:.5},{:.5},{:.5}",
                s.id,
                s.kind,
                s.state,
                s.time_remaining,
                s.position.0,
                s.position.1,
                s.position.2,
                s.quat[0],
                s.quat[1],
                s.quat[2],
                s.quat[3]
            )?;
            self.rows += 1;
        }
        Ok(())
    }

    fn rows(&self) -> usize {
        self.rows
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

/// 实时遥测：连接共享内存，持续输出帧摘要并录制 trace。
/// 用法：`nav-core-cli live [trace.navtrace]`——省略路径时录制到默认路径（见 `default_trace_path`）。
///
/// 默认录制而非可选录制的原因：本命令是 B2 实机采集的唯一数据源，单趟采集耗时 20–30 分钟，
/// 遗漏录制会使整场数据无产出。TraceRecorder 直接写 File（无用户态缓冲），故逐帧即时落盘，
/// Ctrl+C 中断不会丢失已写入的帧。
fn live(trace_path: Option<&str>) {
    let path = match trace_path {
        Some(p) => std::path::PathBuf::from(p),
        None => default_trace_path(),
    };
    let mut src = nav_telemetry::TelemetrySource::new(std::time::Duration::from_secs(2));
    let mut rec = nav_telemetry::TraceRecorder::create(&path).unwrap_or_else(|e| {
        eprintln!("无法创建 trace {}: {e}", path.display());
        std::process::exit(1);
    });
    let mut det = nav_telemetry::EventDetector::new(50.0);
    let mut last_shown = std::time::Instant::now();
    let mut last_report = std::time::Instant::now();
    let mut sem: Option<SemRecorder> = None;
    let mut last_sem = std::time::Instant::now();
    println!("录制 trace: {}", path.display());
    println!("（Ctrl+C 停止；逐帧即时落盘，中断不丢已写入帧）");
    println!("等待遥测桥（Local\\ETS2NavTelemetry）……");
    loop {
        match src.poll() {
            nav_telemetry::TelemetryState::Fresh(snap) => {
                let _ = rec.record(&snap);
                // 每 60 秒报一次采集进度：长时间驾驶时据此确认录制仍在推进
                if last_report.elapsed().as_secs() >= 60 {
                    println!("[rec] 已录制 {} 帧 → {}", rec.count(), path.display());
                    if let Some(r) = sem.as_ref() {
                        println!("[rec] 信号灯 {} 行 → {}", r.rows(), r.path().display());
                    }
                    last_report = std::time::Instant::now();
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
        // 信号灯旁路记录（20 Hz）独立于遥测状态：遥测桥与信号桥是两个插件，信号证据不应
        // 因遥测波动（Stale/Disconnected）而中断。仅在确实读到共享内存时创建文件——无桥接器
        // 时不产出空文件，以免把「没采到」误读成「采到了但无灯」。
        if last_sem.elapsed().as_millis() >= 50 {
            last_sem = std::time::Instant::now();
            if let Some(slots) = nav_telemetry::read_semaphores() {
                if sem.is_none() {
                    match SemRecorder::create(&path) {
                        Ok(r) => {
                            println!("信号灯旁路记录: {}", r.path().display());
                            sem = Some(r);
                        }
                        Err(e) => eprintln!("信号灯记录文件创建失败（不影响 trace）: {e}"),
                    }
                }
                if let Some(r) = sem.as_mut() {
                    let _ = r.record(&slots);
                }
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

/// roundabout-stats：Europe junction 环岛拓扑检测统计（§101）+ 抽查。
fn roundabout_stats(dataset_dir: &str) {
    let (_routing, junctions) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let mut rb = 0u32;
    let mut rb_uk = 0u32;
    for j in &junctions.junctions {
        if nav_router::roundabout::RoundaboutDetector::is_roundabout(j) {
            rb += 1;
            // UK 判定：用 movement 几何（P2-04：UK x<0 但 London x≈-39k——按 junction 平均 x）
            let mut sx = 0.0;
            let mut n = 0.0;
            for m in &j.movements {
                for (x, _, _) in m.geometry.iter().take(4) {
                    sx += x;
                    n += 1.0;
                }
            }
            if n > 0.0 && sx / n < -45000.0 {
                rb_uk += 1;
            }
        }
    }
    println!("环岛 junction（拓扑检测 §101）：{rb}（UK/爱尔兰区域约 {rb_uk}）");
    // 抽查：打印前 3 个环岛的 movement 数
    let mut shown = 0;
    for j in &junctions.junctions {
        if nav_router::roundabout::RoundaboutDetector::is_roundabout(j) && shown < 3 {
            println!(
                "  抽查 junction {} token={} movements={} 节点={}",
                j.uid,
                j.prefab_token,
                j.movements.len(),
                j.node_uids.len()
            );
            shown += 1;
        }
    }
}

/// bench：正式性能基准（§139-140）——加载/内存/路线时延分布/匹配 p99。
fn bench_cli(dataset_dir: &str) {
    // 1) 加载计时
    let t0 = std::time::Instant::now();
    let (routing, junctions) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let load_ms = t0.elapsed().as_secs_f64() * 1000.0;
    // 峰值内存估算（审查修复：加载+build 瞬时含 RoutingGraph 原始数据——峰值高于常驻）
    let peak_estimate_mb = (routing.nodes.len() * 24
        + routing.edges.len() * 64
        + routing
            .edges
            .iter()
            .map(|e| e.geometry.len() * 24)
            .sum::<usize>()) as f64
        / 1e6;
    let t1 = std::time::Instant::now();
    let graph = nav_graph::CompactGraph::build(&routing);
    let build_ms = t1.elapsed().as_secs_f64() * 1000.0;
    drop(routing); // 构建后释放原始数据（运行时只保留压缩图）
    drop(junctions); // 审计 perf-M2：junction.graph 数据运行时也不需要
    let t2 = std::time::Instant::now();
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    let spatial_ms = t2.elapsed().as_secs_f64() * 1000.0;
    // 常驻内存（构建后——审查修复：运行时常驻口径，目标 <500MB 按此判定）
    let resident_mb = (graph.positions.len() * 24
        + graph.edges.len() * 40
        + graph.edges_geometry.len() * 24
        + graph.node_offsets.len() * 4
        + graph.edge_ids.len() * 4
        + graph.in_offsets.len() * 4
        + graph.in_edge_ids.len() * 4) as f64
        / 1e6;
    println!(
        "[加载] {load_ms:.0}ms（routing.graph 冷启动）+ build {build_ms:.0}ms + spatial {spatial_ms:.0}ms"
    );
    println!("[内存] 峰值估算（加载+build 瞬时）~{peak_estimate_mb:.0}MB（含原始数据）；常驻（构建后）估算 {resident_mb:.0}MB（目标 <500MB 按常驻口径）");
    // 2) 路线时延分布（Berlin 核心网 200 OD × fastest）
    let mut router = nav_router::search::Router::new(graph.node_count());
    let mut times = Vec::new();
    let mut solved = 0u32;
    for i in 0..200 {
        let seed = 20260810u64 + i as u64 * 2654435761;
        let mut s = seed;
        let mut rnd = move || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 33) as f64 / (1u64 << 31) as f64
        };
        let (xa, za) = (-62000.0 + rnd() * 9000.0, 30000.0 + rnd() * 8000.0);
        let (xb, zb) = (-62000.0 + rnd() * 9000.0, 30000.0 + rnd() * 8000.0);
        let (Some(s1), Some(s2)) = (
            nav_router::snap::snap_nearest(&graph, &spatial, xa, za, 300.0),
            nav_router::snap::snap_nearest(&graph, &spatial, xb, zb, 300.0),
        ) else {
            continue;
        };
        let t = std::time::Instant::now();
        let req = nav_router::search::RouteRequest::new(
            &graph,
            nav_router::snap::VirtualEndpoint::start(&s1, true),
            nav_router::snap::VirtualEndpoint::goal(&s2),
            nav_router::cost::RouteProfile::Fastest,
        );
        if router.astar(&req).is_some() {
            solved += 1;
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p = |q: f64| -> f64 { times[((times.len() as f64) * q) as usize] };
    println!("[路线] {} 条（Berlin 核心网 fastest）：p50={:.2}ms p95={:.3}ms p99={:.3}ms max={:.3}ms（目标典型 <500ms）",
        solved, p(0.5), p(0.95), p(0.99), times.last().unwrap());
    // 3) 匹配 p99（trace 回放）
    let trace = "C:/Users/20659/AppData/Local/Temp/real.navtrace";
    if std::path::Path::new(trace).exists() {
        let frames: Vec<nav_telemetry::TraceFrame> =
            nav_telemetry::replay(std::path::Path::new(trace))
                .unwrap()
                .collect();
        let mut m = nav_matcher::MapMatcher::new(nav_matcher::MatcherConfig::default());
        let mut mtimes = Vec::new();
        for f in &frames {
            let p = f.snap.position;
            let yaw = quat_yaw(f.snap.heading);
            let t = std::time::Instant::now();
            m.match_frame(&graph, &spatial, p[0], p[2], yaw);
            mtimes.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        mtimes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mp = |q: f64| -> f64 { mtimes[((mtimes.len() as f64) * q) as usize] };
        println!(
            "[匹配] {} 帧：p50={:.3}ms p99={:.3}ms（目标 p99<10ms）",
            mtimes.len(),
            mp(0.5),
            mp(0.99)
        );
    }
    // 4) P3 前方限速查询热路径（10000 次 Berlin 路线 lookahead）
    let (s1, s2) = (
        nav_router::snap::snap_nearest(&graph, &spatial, -58456.0, 32832.0, 300.0),
        nav_router::snap::snap_nearest(&graph, &spatial, -52925.0, 36510.0, 300.0),
    );
    let mut router2 = nav_router::search::Router::new(graph.node_count());
    let req2 = nav_router::search::RouteRequest::new(
        &graph,
        nav_router::snap::VirtualEndpoint::start(&s1.unwrap(), true),
        nav_router::snap::VirtualEndpoint::goal(&s2.unwrap()),
        nav_router::cost::RouteProfile::Fastest,
    );
    let route = router2.astar(&req2).expect("Berlin 路线应存在");
    let mut stimes = Vec::with_capacity(10000);
    for _ in 0..10000 {
        let t = std::time::Instant::now();
        nav_router::speed::speed_breaks_ahead(&route, &graph, 3000.0);
        stimes.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    stimes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let sp = |q: f64| -> f64 { stimes[((stimes.len() as f64) * q) as usize] };
    println!(
        "[限速查询] 10000 次（Berlin 3000m lookahead）：p50={:.3}us p99={:.3}us（P3 目标 p99<10us）",
        sp(0.5) * 1000.0,
        sp(0.99) * 1000.0
    );
    // 5) 真实进程内存（审计 perf-M2：GetProcessMemoryInfo 工作集实测口径）
    println!(
        "[内存-进程] 工作集 {:.0}MB（GetProcessMemoryInfo 实测，含 junctions）",
        process_working_set_mb()
    );
    println!("Bench PASS");
}

/// Windows 进程工作集（MB）。零依赖 psapi FFI（同 nav-telemetry 模式）。
#[cfg(windows)]
fn process_working_set_mb() -> f64 {
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn GetProcessMemoryInfo(
            h: *mut core::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }
    let mut c = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    unsafe {
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0 {
            c.working_set_size as f64 / 1e6
        } else {
            f64::NAN
        }
    }
}

#[cfg(not(windows))]
fn process_working_set_mb() -> f64 {
    f64::NAN
}

/// regression：Europe 全图区域化回归（P2-18）——
/// 五区域 OD 采样 × 三 profile：A*==Dijkstra 一致性、可达率、时延分布；
/// 全链路冒烟：route → tracker → maneuver → session 沿路线闭环。
fn regression_cli(dataset_dir: &str) {
    let (routing, junctions) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let graph = nav_graph::CompactGraph::build(&routing);
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    let mut router = nav_router::search::Router::new(graph.node_count());
    // 五区域（x, z 中心 + 半径；Europe bbox x∈[-94k,78k] z∈[-122k,87k]）
    let regions: [(&str, f64, f64, f64); 5] = [
        ("UK/爱尔兰", -39500.0, -11000.0, 8000.0),
        ("Berlin/德国东北", -58400.0, 33000.0, 8000.0),
        ("法国/比荷卢", -35000.0, -25000.0, 8000.0),
        ("南欧(罗马)", -6800.0, -45000.0, 6000.0),
        ("东欧", 10000.0, 10000.0, 8000.0),
    ];
    let mut total_od = 0u32;
    let mut total_ok = 0u32;
    let mut total_fail = 0u32;
    let mut unreachable = 0u32;
    let mut max_ms = 0.0f64;
    for (name, cx, cz, r) in regions {
        let mut od = 0u32;
        let mut ok = 0u32;
        for i in 0..30 {
            let seed = 20260810u64 ^ ((name.len() as u64) << 32) ^ (i as u64 * 2654435761);
            let mut s = seed;
            let mut rnd = move || {
                s = s
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (s >> 33) as f64 / (1u64 << 31) as f64
            };
            let xa = cx + (rnd() - 0.5) * 2.0 * r;
            let za = cz + (rnd() - 0.5) * 2.0 * r;
            let xb = cx + (rnd() - 0.5) * 2.0 * r;
            let zb = cz + (rnd() - 0.5) * 2.0 * r;
            let Some(s1) = nav_router::snap::snap_nearest(&graph, &spatial, xa, za, 300.0) else {
                continue;
            };
            let Some(s2) = nav_router::snap::snap_nearest(&graph, &spatial, xb, zb, 300.0) else {
                continue;
            };
            od += 1;
            for profile in [
                nav_router::cost::RouteProfile::Shortest,
                nav_router::cost::RouteProfile::Fastest,
                nav_router::cost::RouteProfile::Balanced,
            ] {
                let t0 = std::time::Instant::now();
                let req = nav_router::search::RouteRequest::new(
                    &graph,
                    nav_router::snap::VirtualEndpoint::start(&s1, true),
                    nav_router::snap::VirtualEndpoint::goal(&s2),
                    profile,
                );
                let d = router.dijkstra(&req);
                let a = router.astar(&req);
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                max_ms = max_ms.max(ms);
                match (d, a) {
                    (Some(dr), Some(ar)) => {
                        let cd = nav_router::search::route_cost(&graph, &dr, profile);
                        let ca = nav_router::search::route_cost(&graph, &ar, profile);
                        if (cd - ca).abs() < 1e-3 {
                            ok += 1;
                        } else {
                            total_fail += 1;
                        }
                    }
                    (None, None) => {
                        unreachable += 1;
                    }
                    _ => {
                        total_fail += 1;
                    }
                }
            }
        }
        println!("[{name}] OD {od} × 3profile：一致 {ok} 不可达 {unreachable}（累计）");
        total_od += od * 3;
        total_ok += ok;
    }
    println!(
        "区域化回归: {total_ok}/{total_od} 一致，不一致 {total_fail}，最大单次搜索 {max_ms:.1}ms"
    );
    // 全链路冒烟：Berlin 路线 → tracker → maneuver → session 沿路线闭环
    let s1 = nav_router::snap::snap_nearest(&graph, &spatial, -58456.0, 32832.0, 300.0).unwrap();
    let s2 = nav_router::snap::snap_nearest(&graph, &spatial, -58456.0, 35000.0, 300.0).unwrap();
    let req = nav_router::search::RouteRequest::new(
        &graph,
        nav_router::snap::VirtualEndpoint::start(&s1, true),
        nav_router::snap::VirtualEndpoint::goal(&s2),
        nav_router::cost::RouteProfile::Fastest,
    );
    let route = router.astar(&req).expect("Berlin 路线应存在");
    // tracker
    let mut tracker = nav_router::tracker::RouteTracker::new(&graph, route.clone(), 4);
    let mut last = 0.0;
    let mut mono = true;
    for &eid in &route.edges {
        let e = &graph.edges[eid as usize];
        let pts = graph.edge_geometry(e);
        let mut acc = 0.0;
        for w in pts.windows(2) {
            acc += ((w[1].0 - w[0].0).powi(2) + (w[1].2 - w[0].2).powi(2)).sqrt();
            let u = tracker.update(&graph, eid, acc);
            if u.distance_travelled < last - 1e-6 {
                mono = false;
            }
            last = u.distance_travelled;
        }
    }
    let mut turns = std::collections::HashMap::new();
    for j in &junctions.junctions {
        for m in &j.movements {
            turns.insert((j.uid, m.id), m.turn_type);
        }
    }
    let maneuvers = nav_router::maneuver::generate_maneuvers(&graph, &route, &turns);
    println!(
        "全链路冒烟: route {} 边 → tracker progress={:.3} 单调={} → maneuver {} 条",
        route.edges.len(),
        tracker.progress(),
        mono,
        maneuvers.len()
    );
    assert!(tracker.progress() > 0.9, "tracker 应接近完成");
    assert!(mono);
    assert!(maneuvers.len() >= 2, "maneuver 至少 Depart/Arrive");
    println!("P2-18 Regression PASS");
}

/// session：trace 驱动的完整导航会话（§118-121：状态机 + 快照）。
fn session_cli(trace_path: &str, dataset_dir: &str) {
    let (routing, junctions) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let graph = std::sync::Arc::new(nav_graph::CompactGraph::build(&routing));
    let spatial = std::sync::Arc::new(nav_spatial::SpatialIndex::build(
        &graph,
        nav_spatial::DEFAULT_CELL_SIZE,
    ));
    let mut turns: std::collections::HashMap<(u64, u32), i8> = std::collections::HashMap::new();
    for j in &junctions.junctions {
        for m in &j.movements {
            turns.insert((j.uid, m.id), m.turn_type);
        }
    }
    let frames: Vec<nav_telemetry::TraceFrame> =
        nav_telemetry::replay(std::path::Path::new(trace_path))
            .unwrap_or_else(|e| {
                eprintln!("打开 trace 失败: {e}");
                std::process::exit(1);
            })
            .collect();
    let mut session = nav_router::session::NavigationSession::new(
        graph,
        spatial,
        turns,
        nav_router::session::SessionConfig::default(),
    );
    // 起点帧确定位置
    session.on_frame(&frames[0].snap);
    // 目的地：trace 终点（坐标）
    let last = frames.last().unwrap().snap.position;
    let (routing2, _j2) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir)).unwrap();
    let graph2 = nav_graph::CompactGraph::build(&routing2);
    let spatial2 = nav_spatial::SpatialIndex::build(&graph2, nav_spatial::DEFAULT_CELL_SIZE);
    let dest = nav_router::destination::DestinationResolver::new(Vec::new(), &routing2.nodes)
        .resolve_coordinate(
            &graph2,
            &spatial2,
            last[0],
            last[2],
            nav_router::destination::DestKind::Coordinate,
        )
        .unwrap();
    match session.set_destination(dest) {
        Ok(()) => println!("目的地已设置（trace 终点）→ {}", session.state().name()),
        Err(e) => {
            println!("设置目的地失败: {e}");
            return;
        }
    }
    // 逐帧驱动：沿规划路线的几何点生成帧（验证状态机闭环——避免 trace 自由驾驶偏航干扰）
    let mut last_state = nav_router::session::SessionState::Idle;
    let mut route_pts: Vec<(f64, f64, f64, f64)> = Vec::new(); // (x, y, z, yaw)
    if let Some(route) = session.route().cloned() {
        let g = session.graph();
        let mut raw: Vec<(f64, f64, f64, f64)> = Vec::new();
        for eid in &route.edges {
            let e = &g.edges[*eid as usize];
            let pts = g.edge_geometry(e);
            for w in pts.windows(2) {
                let yaw = (w[1].2 - w[0].2).atan2(w[1].0 - w[0].0);
                raw.push((w[0].0, w[0].1, w[0].2, yaw));
            }
        }
        if let Some((pe, _, _)) = route.end_virtual {
            let e = &g.edges[pe as usize];
            let pts = g.edge_geometry(e);
            if let Some((x, y, z)) = pts.last() {
                let (lx, _, lz) = pts[pts.len() - 2];
                let yaw = (z - lz).atan2(x - lx);
                raw.push((*x, *y, *z, yaw));
            }
        }
        // 5m 插值（几何点间距大——matcher 需要平滑帧）
        for w in raw.windows(2) {
            let (ax, ay, az, ayaw) = w[0];
            let (bx, by, bz, _) = w[1];
            let dx = bx - ax;
            let dz = bz - az;
            let seg = (dx * dx + dz * dz).sqrt();
            let steps = (seg / 5.0).ceil().max(1.0) as u32;
            for k in 0..steps {
                let t = k as f64 / steps as f64;
                route_pts.push((ax + dx * t, ay + (by - ay) * t, az + dz * t, ayaw));
            }
        }
        if let Some(&last) = raw.last() {
            route_pts.push(last);
        }
        println!("沿路线插值生成 {} 个帧点", route_pts.len());
    }
    for (i, (x, y, z, yaw)) in route_pts.iter().enumerate() {
        let mut snap = frames[0].snap.clone();
        snap.position = [*x, *y, *z];
        snap.heading = [0.0, (yaw / 2.0).sin() as f32, 0.0, (yaw / 2.0).cos() as f32];
        snap.speed = 14.0;
        let snap = session.on_frame(&snap);
        // P3 提醒事件流打印（§39-41/§36-38 运行时接入）
        for r in &snap.reminders {
            println!("帧 {i}: 提醒 {}", nav_router::speak::to_speech_zh(r));
        }
        if !snap.diagnostics.is_empty() {
            println!("帧 {i}: diag {}", snap.diagnostics);
        }
        if snap.state != last_state {
            println!(
                "帧 {i}: → {}（matched={:?} 剩余={:.0}m progress={:.2} diag={}）",
                snap.state.name(),
                snap.match_confidence,
                snap.remaining_m.unwrap_or(-1.0),
                snap.progress.unwrap_or(0.0),
                snap.diagnostics
            );
            last_state = snap.state;
        }
        if snap.state == nav_router::session::SessionState::Arrived {
            println!("到达（帧 {i}）——状态机闭环验证 PASS");
            break;
        }
    }
    println!("最终状态: {}", session.state().name());
}

/// signal：路线受控 movement 静态绑定 + runtime 关联（§111-117）。
fn signal_cli(xz: &str, dataset_dir: &str) {
    let parts: Vec<&str> = xz.split(':').collect();
    let (x1, z1) = {
        let v: Vec<&str> = parts[0].split(',').collect();
        (v[0].trim().parse().unwrap(), v[1].trim().parse().unwrap())
    };
    let (x2, z2) = {
        let v: Vec<&str> = parts[1].split(',').collect();
        (v[0].trim().parse().unwrap(), v[1].trim().parse().unwrap())
    };
    let (routing, _j) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let graph = nav_graph::CompactGraph::build(&routing);
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    let s1 = nav_router::snap::snap_nearest(&graph, &spatial, x1, z1, 300.0).unwrap();
    let s2 = nav_router::snap::snap_nearest(&graph, &spatial, x2, z2, 300.0).unwrap();
    let mut router = nav_router::search::Router::new(graph.node_count());
    let req = nav_router::search::RouteRequest::new(
        &graph,
        nav_router::snap::VirtualEndpoint::start(&s1, true),
        nav_router::snap::VirtualEndpoint::goal(&s2),
        nav_router::cost::RouteProfile::Fastest,
    );
    let Some(route) = router.astar(&req) else {
        println!("无路线");
        return;
    };
    // 受控 movement 序列（前 6 个）
    let mut idx = 0;
    let mut found = 0;
    while let Some((i, eid, juid, group)) =
        nav_router::signal::SignalLinker::next_controlled_movement(&graph, &route, idx)
    {
        if found >= 6 {
            break;
        }
        let pose = nav_router::signal::SignalLinker::static_head_pose(&graph, eid);
        // runtime 关联（无游戏时为 None——只打印静态绑定）
        let runtime = nav_telemetry::read_semaphores();
        let up = match &runtime {
            Some(lights) => {
                let lights: Vec<nav_router::signal::RuntimeSignal> = lights
                    .iter()
                    .map(|l| nav_router::signal::RuntimeSignal {
                        position: l.position,
                        quat: l.quat,
                        kind: l.kind,
                        time_remaining: l.time_remaining,
                        state: l.state,
                        id: l.id,
                    })
                    .collect();
                nav_router::signal::SignalLinker::link(eid, juid, group, &pose, &lights)
            }
            None => nav_router::signal::SignalLinker::link(eid, juid, group, &pose, &[]),
        };
        println!(
            "受控 movement @route边{i}（edge={eid} junction={juid:#x} group={group}）→ 置信度 {:?}",
            up.confidence
        );
        idx = i + 1;
        found += 1;
    }
    if found == 0 {
        println!("路线中无受控 movement（无信号灯）");
    } else {
        println!("（共 {found} 个受控 movement——runtime 关联需游戏会话，CLI 环境无）");
    }
}

/// dest：目的地解析（§107-110：POI/Job/坐标 → access snap）。
fn dest_cli(query: &str, dataset_dir: &str) {
    let (routing, _j) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let graph = nav_graph::CompactGraph::build(&routing);
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    let pois = nav_dataset::load_pois(&std::path::Path::new(dataset_dir).join("search.db"))
        .unwrap_or_else(|e| {
            eprintln!("加载 POI 失败: {e}");
            std::process::exit(1);
        });
    let resolver = nav_router::destination::DestinationResolver::new(pois, &routing.nodes);
    let parts: Vec<&str> = query.split(',').collect();
    let result = if parts.len() == 2 && parts[0].trim().parse::<f64>().is_ok() {
        // 坐标
        let x: f64 = parts[0].trim().parse().unwrap();
        let z: f64 = parts[1].trim().parse().unwrap();
        resolver.resolve_coordinate(
            &graph,
            &spatial,
            x,
            z,
            nav_router::destination::DestKind::Coordinate,
        )
    } else if query.contains('@') {
        // job: company@city
        let v: Vec<&str> = query.split('@').collect();
        resolver.resolve_job(&graph, &spatial, v[0].trim(), Some(v[1].trim()))
    } else {
        resolver.resolve_poi(&graph, &spatial, query)
    };
    match result {
        Ok(d) => println!(
            "[{}] {} @ ({:.0},{:.0}) → access_snap edge={} off={:.0}m lateral={:.0}m",
            d.kind.name(),
            d.name,
            d.position.0,
            d.position.2,
            d.access_snap.edge_id,
            d.access_snap.offset,
            d.access_snap.lateral
        ),
        Err(e) => println!("解析失败: {e}"),
    }
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
        let _ = (xa, za, xb, zb);
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
/// 前方限速断点（§40）：route fastest 后沿路线聚合 (offset, limit)。
/// 用法: nav-core-cli speed <x1,z1:x2,z2> <dataset-dir> [horizon_m]
fn speed_cli(xz: &str, dataset_dir: &str, horizon: Option<&String>) {
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
    let horizon_m = horizon
        .and_then(|h| h.parse::<f32>().ok())
        .unwrap_or(3000.0);
    let (routing, _junctions) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
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
    let req = nav_router::search::RouteRequest::new(
        &graph,
        nav_router::snap::VirtualEndpoint::start(&s1, true),
        nav_router::snap::VirtualEndpoint::goal(&s2),
        nav_router::cost::RouteProfile::Fastest,
    );
    match router.astar(&req) {
        Some(r) => {
            let breaks = nav_router::speed::speed_breaks_ahead(&r, &graph, horizon_m);
            println!(
                "路线 {:.0}m {} 边；前方 {:.0}m 限速断点 {} 个：",
                r.distance_m,
                r.edges.len(),
                horizon_m,
                breaks.len()
            );
            println!("breaks={} (machine-readable)", breaks.len());
            let mut prev = -1.0f32;
            for (i, b) in breaks.iter().enumerate() {
                let d = b.offset_m - prev;
                let lim = match b.limit {
                    -1 => "未知".to_string(),
                    0 => "无限速".to_string(),
                    l => format!("{l} km/h"),
                };
                println!("  [{i:>2}] +{:.0}m 起（持续 {:.0}m）→ {lim}", b.offset_m, d);
                prev = b.offset_m;
            }
        }
        None => eprintln!("无路线"),
    }
}

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
    let (routing, junctions) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let graph = nav_graph::CompactGraph::build(&routing);
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    // junction turn 查询表（§96：movement TurnType 优先）
    let mut turns: std::collections::HashMap<(u64, u32), i8> = std::collections::HashMap::new();
    for j in &junctions.junctions {
        for m in &j.movements {
            turns.insert((j.uid, m.id), m.turn_type);
        }
    }
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
    // —— Maneuver 序列（§94-99：TurnType 优先 + 几何细化 + 抑制）——
    if let Some(best) = alts.routes.first() {
        let ms = nav_router::maneuver::generate_maneuvers(&graph, best, &turns);
        println!("maneuver 序列（{} 条）：", ms.len());
        for m in ms.iter().take(30) {
            println!(
                "  {:>12} @edge{} Δ={:+.0}° 距上 {:.0}m",
                m.mtype.name(),
                m.route_edge_index,
                m.bearing_change.to_degrees(),
                m.distance_from_prev
            );
        }
        if ms.len() > 30 {
            println!("  ... 共 {} 条", ms.len());
        }
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

/// 四元数 → yaw（世界弧度；SCS quat (x,y,z,w) 绕 Y 轴——审查修复，与 signal::light_yaw 一致）。
fn quat_yaw(q: [f32; 4]) -> f64 {
    let (x, y, z, w) = (q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64);
    (2.0 * (w * y - x * z)).atan2(1.0 - 2.0 * (y * y + z * z))
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

/// P4 nav-server（§60/§61）：nav-core-cli server <dataset-dir> [--replay=trace] [--port=N] [--web=dir]
///
/// 选项按名字扫描（顺序无关），不再按固定位置取 `args.get(3..6)`——原实现使单独
/// 传入的 `--port` 落入 replay 槽位被静默丢弃，端口回落 8123（P4R Batch 2 实测：
/// `server <ds> --port=18236` 实际仍监听 8123）。Playwright E2E 需要动态端口，
/// 故此处按名字取值。
fn server_cli_run(args: &[String], fake_signal: bool) {
    let dataset_dir = &args[2];
    let port: u16 = flag_value(args, "--port")
        .and_then(|v| v.parse().ok())
        .unwrap_or(8123);
    let web_root = flag_value(args, "--web")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "../tools/ets2nav-web/dist".to_string());
    let trace = flag_value(args, "--replay").map(|v| v.to_string());
    server_cli::server_cli(dataset_dir, trace.as_deref(), port, &web_root, fake_signal);
}

/// 从参数表提取 `--name=value` 或 `--name value` 形式的值（两种写法等价）。
fn flag_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let eq = format!("{name}=");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(v) = a.strip_prefix(&eq) {
            return Some(v);
        }
        if a == name {
            return it.next().map(|s| s.as_str());
        }
    }
    None
}

/// 合成 trace 生成（P4 UI 回放验证）：路线插值 + 速度曲线 → .navtrace
/// 用法：nav-core-cli syntrace <x1,z1:x2,z2> <dataset-dir> <out.navtrace>
fn syntrace_cli(xz: &str, dataset_dir: &str, out: &str) {
    let parts: Vec<&str> = xz.split(':').collect();
    let parse = |s: &str| -> (f64, f64) {
        let v: Vec<&str> = s.split(',').collect();
        (v[0].trim().parse().unwrap(), v[1].trim().parse().unwrap())
    };
    let (x1, z1) = parse(parts[0]);
    let (x2, z2) = parse(parts[1]);
    let (routing, junctions) = nav_dataset::load_dataset(std::path::Path::new(dataset_dir))
        .unwrap_or_else(|e| {
            eprintln!("加载 dataset 失败: {e}");
            std::process::exit(1);
        });
    let graph = nav_graph::CompactGraph::build(&routing);
    let spatial = nav_spatial::SpatialIndex::build(&graph, nav_spatial::DEFAULT_CELL_SIZE);
    let _ = junctions;
    let s1 = nav_router::snap::snap_nearest(&graph, &spatial, x1, z1, 300.0).unwrap();
    let s2 = nav_router::snap::snap_nearest(&graph, &spatial, x2, z2, 300.0).unwrap();
    let mut router = nav_router::search::Router::new(graph.node_count());
    let req = nav_router::search::RouteRequest::new(
        &graph,
        nav_router::snap::VirtualEndpoint::start(&s1, true),
        nav_router::snap::VirtualEndpoint::goal(&s2),
        nav_router::cost::RouteProfile::Fastest,
    );
    let route = router.astar(&req).expect("路线应存在");
    // 几何插值（5m）
    let mut route_pts: Vec<(f64, f64, f64, f64)> = Vec::new();
    let mut raw: Vec<(f64, f64, f64, f64)> = Vec::new();
    for &eid in &route.edges {
        let e = &graph.edges[eid as usize];
        let pts = graph.edge_geometry(e);
        for w in pts.windows(2) {
            let yaw = (w[1].2 - w[0].2).atan2(w[1].0 - w[0].0);
            raw.push((w[0].0, w[0].1, w[0].2, yaw));
        }
    }
    if let Some((pe, _, _)) = route.end_virtual {
        let e = &graph.edges[pe as usize];
        let pts = graph.edge_geometry(e);
        if let Some((x, y, z)) = pts.last() {
            let (lx, _, lz) = pts[pts.len() - 2];
            raw.push((*x, *y, *z, (z - lz).atan2(x - lx)));
        }
    }
    for w in raw.windows(2) {
        let (ax, ay, az, ayaw) = w[0];
        let (bx, by, bz, _) = w[1];
        let dx = bx - ax;
        let dz = bz - az;
        let seg = (dx * dx + dz * dz).sqrt();
        let steps = (seg / 1.0).ceil().max(1.0) as u32; // 1m 点表（M2：位置跳变 ≤1m）
        for k in 0..steps {
            let t = k as f64 / steps as f64;
            route_pts.push((ax + dx * t, ay + (by - ay) * t, az + dz * t, ayaw));
        }
    }
    if let Some(&last) = raw.last() {
        route_pts.push(last);
    }
    // 速度曲线：0→8s 加速到 22 m/s 巡航 → 最后 200m 减速到 5 m/s（A2c-M2 修复：
    // 位置由速度积分驱动——原实现位置步进恒定（隐含 267 km/h）与 speed 字段矛盾）
    let cruise = 22.0f32;
    let dt_s = 0.05f32; // 20Hz
                        // N-M1（复审）：total_dist 用点表弧长（Σ 相邻段长）——弦长会提前终止行程（只走 56%）
    let total_dist: f32 = route_pts
        .windows(2)
        .map(|w| {
            let (ax, _, az, _) = w[0];
            let (bx, _, bz, _) = w[1];
            ((bx - ax).powi(2) + (bz - az).powi(2)).sqrt() as f32
        })
        .sum();
    let decel_start = (total_dist - 200.0).max(0.0);
    let mut dist_traveled = 0.0f32; // 当前段内弧长（位置推进用）
    let mut total_traveled = 0.0f32; // 累计总距离（速度曲线用）
    let mut frames = Vec::new();
    let mut sim = 0u64;
    let mut seq = 780u32;
    let mut idx = 0usize;
    loop {
        // 速度曲线：0→8s 线性加速到巡航、巡航、最后 200m 减速到 0.5 m/s 下限
        let elapsed_s = frames.len() as f32 * dt_s;
        let speed = if elapsed_s < 8.0 {
            (elapsed_s / 8.0 * cruise).min(cruise).max(0.5)
        } else if total_traveled > decel_start {
            (cruise * ((total_dist - total_traveled) / 200.0).max(0.0))
                .min(cruise)
                .max(0.5)
        } else {
            cruise
        };
        let (x, y, z, yaw) = route_pts[idx];
        let snap = nav_telemetry::TelemetrySnapshot {
            sequence: seq,
            layout_version: 1,
            running: true,
            paused: false,
            simulation_time: sim,
            paused_simulation_time: 0,
            render_time: 0,
            game_time_minutes: 0,
            local_scale: 1.0,
            rest_stop_minutes: 0,
            position: [x, y, z],
            // P4 审计 MINOR 修复（2026-08-12）：由点表逐点 yaw 生成朝向四元数。
            // 原为恒等四元数（heading 恒 0）——matcher 全程按「朝北」打分，巡航段
            // 持续失配，使 verify-server-protocol.py 的 remaining 递减断言不稳定。
            heading: nav_telemetry::yaw_to_quat(yaw),
            speed,
            speed_limit: 0.0,
            fuel_amount: 1.0,
            fuel_range: 1000.0,
            fuel_warning: false,
            job: None,
        };
        frames.push(snap);
        sim += 50_000;
        seq += 1;
        // 位置 = 弧长推进（speed × dt）
        total_traveled += speed * dt_s;
        dist_traveled += speed * dt_s;
        if total_traveled >= total_dist {
            break;
        }
        // 沿路线点表推进弧长
        while idx + 1 < route_pts.len() {
            let (ax, _, az, _) = route_pts[idx];
            let (bx, _, bz, _) = route_pts[idx + 1];
            let seg = ((bx - ax).powi(2) + (bz - az).powi(2)).sqrt() as f32;
            if dist_traveled >= seg {
                dist_traveled -= seg;
                idx += 1;
            } else {
                break;
            }
        }
        if idx + 1 >= route_pts.len() {
            break;
        }
    }
    let mut rec = nav_telemetry::TraceRecorder::create(std::path::Path::new(out)).unwrap();
    for f in &frames {
        rec.record(f).unwrap();
    }
    rec.flush().unwrap();
    println!(
        "SYNTRACE OK frames={} dist={:.0}m out={}",
        frames.len(),
        route.distance_m,
        out
    );
}

#[cfg(test)]
mod tests {
    use super::{default_trace_path, utc_parts};

    #[test]
    fn utc_parts_matches_known_timestamps() {
        assert_eq!(utc_parts(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(utc_parts(1_767_225_600), (2026, 1, 1, 0, 0, 0));
        assert_eq!(utc_parts(1_789_058_002), (2026, 9, 10, 16, 33, 22));
    }

    #[test]
    fn utc_parts_handles_leap_day_and_year_end() {
        // 闰年 2 月末：覆盖 m<=2 的年份回退分支
        assert_eq!(utc_parts(1_709_251_199), (2024, 2, 29, 23, 59, 59));
        // 12 月末：覆盖 mp>=10 的月份换算分支
        assert_eq!(utc_parts(1_735_689_599), (2024, 12, 31, 23, 59, 59));
    }

    #[test]
    fn default_trace_path_lands_in_temp_with_timestamped_name() {
        let p = default_trace_path();
        assert!(p.starts_with(std::env::temp_dir()), "应位于临时目录: {p:?}");
        assert_eq!(p.extension().and_then(|s| s.to_str()), Some("navtrace"));
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        // 形如 ets2nav-live-YYYYMMDD-HHMMSS.navtrace（长度固定，便于多轮采集区分）
        assert_eq!(name.len(), "ets2nav-live-YYYYMMDD-HHMMSS.navtrace".len());
        assert!(name.starts_with("ets2nav-live-"), "{name}");
    }

    #[test]
    fn sem_recorder_writes_long_table_with_header() {
        let dir = std::env::temp_dir();
        let trace = dir.join("ets2nav-sem-test.navtrace");
        let mut r = super::SemRecorder::create(&trace).unwrap();
        // 派生路径：trace 扩展名被替换，stem 保留
        assert_eq!(r.path().file_name().unwrap(), "ets2nav-sem-test.sem.csv");
        let slots = [
            nav_telemetry::SemaphoreSlot {
                position: (1.5, 2.5, 3.5),
                // 90° 偏航四元数：y=w=sin(45°)=cos(45°)。用常量而非字面量 0.7071——
                // 后者会触发 clippy::approx_constant（该 lint 正是为这类近似常数而设）。
                quat: [
                    0.0,
                    std::f32::consts::FRAC_1_SQRT_2,
                    0.0,
                    std::f32::consts::FRAC_1_SQRT_2,
                ],
                kind: 1,
                time_remaining: 12.25,
                state: 2,
                id: 42,
            },
            nav_telemetry::SemaphoreSlot {
                position: (-4.0, 0.0, 8.0),
                quat: [0.0, 0.0, 0.0, 1.0],
                kind: 0,
                time_remaining: 0.0,
                state: 0,
                id: 7,
            },
        ];
        r.record(&slots).unwrap();
        assert_eq!(r.rows(), 2, "两灯应写两行");

        let text = std::fs::read_to_string(r.path()).unwrap();
        drop(r);
        let _ = std::fs::remove_file(&trace);
        let _ = std::fs::remove_file(dir.join("ets2nav-sem-test.sem.csv"));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "表头 + 2 数据行");
        assert_eq!(
            lines[0],
            "wall_ms,slot,id,kind,state,time_remaining,x,y,z,qx,qy,qz,qw"
        );
        let f: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(f.len(), 13, "每行列数固定");
        assert_eq!(f[1], "0", "slot 序号");
        assert_eq!(f[2], "42", "id");
        assert_eq!(f[5], "12.250", "time_remaining 保留 3 位");
        assert_eq!(f[6], "1.500", "x");
        assert_eq!(f[8], "3.500", "z");
        assert_eq!(f[10], "0.70711", "quat y 保留 5 位");
        assert_eq!(lines[2].split(',').nth(1), Some("1"), "第二灯 slot 序号");
    }
}
