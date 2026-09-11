use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use crate::server::{
    http_reply, http_reply_static, parse_range, read_http_head, snapshot_json, ws_accept_key,
    ws_recv_frame, ws_send_frame, RangeRequest, ServerShared,
};
use nav_router::maneuver::TurnLookup;

// ─── server_cli：HTTP + WS + 数据源线程 ─────────────────────────────────────

pub struct ServerCtx {
    pub graph: std::sync::Arc<nav_graph::CompactGraph>,
    pub spatial: std::sync::Arc<nav_spatial::SpatialIndex>,
    pub dataset_dir: String,
    pub shared: Arc<ServerShared>,
    /// POI 表（/api/search 用；启动时从 search.db 加载——内存过滤）。
    pub pois: Vec<nav_dataset::PoiRecord>,
}

fn content_type(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") {
        "text/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".pmtiles") {
        "application/octet-stream"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/octet-stream"
    }
}

/// 静态文件 + API 路由（HTTP 短连接）。graph 访问经 gate 锁（短暂持有，<1ms 级）。
fn handle_http(
    ctx: &ServerCtx,
    stream: &mut TcpStream,
    req_line: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/").to_string();
    let qpath = path.split('?').next().unwrap_or("/");

    // POST /api/route：{"from":[x,z],"to":[x,z]}
    if method == "POST" && qpath == "/api/route" {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
            return http_reply(
                stream,
                "400 Bad Request",
                "application/json",
                b"{\"error\":\"bad json\"}",
            );
        };
        let from = v["from"]
            .as_array()
            .and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
        let to = v["to"]
            .as_array()
            .and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?)));
        let (Some((fx, fz)), Some((tx, tz))) = (from, to) else {
            return http_reply(
                stream,
                "400 Bad Request",
                "application/json",
                b"{\"error\":\"need from/to [x,z]\"}",
            );
        };
        let graph = &ctx.graph;
        let spatial = &ctx.spatial;
        let Some(s1) = nav_router::snap::snap_nearest(graph, spatial, fx, fz, 300.0) else {
            return http_reply(
                stream,
                "404 Not Found",
                "application/json",
                b"{\"error\":\"start not routable\"}",
            );
        };
        let Some(s2) = nav_router::snap::snap_nearest(graph, spatial, tx, tz, 300.0) else {
            return http_reply(
                stream,
                "404 Not Found",
                "application/json",
                b"{\"error\":\"dest not routable\"}",
            );
        };
        let mut router = nav_router::search::Router::new(graph.node_count());
        let req = nav_router::search::RouteRequest::new(
            graph,
            nav_router::snap::VirtualEndpoint::start(&s1, true),
            nav_router::snap::VirtualEndpoint::goal(&s2),
            nav_router::cost::RouteProfile::Fastest,
        );
        return match router.astar(&req) {
            Some(route) => {
                let mut polyline: Vec<[f64; 2]> = Vec::new();
                for &eid in &route.edges {
                    let e = &graph.edges[eid as usize];
                    let pts = graph.edge_geometry(e);
                    for p in pts {
                        polyline.push([p.0, p.2]);
                    }
                }
                if let Some((pe, _, _)) = route.end_virtual {
                    let e = &graph.edges[pe as usize];
                    let pts = graph.edge_geometry(e);
                    if let Some(p) = pts.last() {
                        polyline.push([p.0, p.2]);
                    }
                }
                let out = serde_json::json!({
                    "distance_m": route.distance_m,
                    "polyline": polyline,
                    "edge_count": route.edges.len(),
                });
                // 同步设置 session 目的地（数据源线程消费——UI 设目的地 → 导航启动）
                *ctx.shared.pending_dest.lock().unwrap() = Some((tx, tz));
                http_reply(
                    stream,
                    "200 OK",
                    "application/json",
                    out.to_string().as_bytes(),
                )
            }
            None => http_reply(
                stream,
                "404 Not Found",
                "application/json",
                b"{\"error\":\"no route\"}",
            ),
        };
    }

    // GET /api/snapshot：轮询备用通道
    if method == "GET" && qpath == "/api/snapshot" {
        let latest = ctx.shared.latest_json.lock().unwrap().clone();
        return http_reply(stream, "200 OK", "application/json", latest.as_bytes());
    }

    // GET /api/metadata
    if method == "GET" && qpath == "/api/metadata" {
        let out = serde_json::json!({
            "nodes": ctx.graph.node_count(),
            "edges": ctx.graph.edges.len(),
            "dataset": ctx.dataset_dir,
        });
        return http_reply(
            stream,
            "200 OK",
            "application/json",
            out.to_string().as_bytes(),
        );
    }

    // GET /api/search?q=xxx（A2a-M1 §60：POI 搜索——search.db 全量加载后内存过滤）
    if method == "GET" && qpath == "/api/search" {
        let q = path
            .split('?')
            .nth(1)
            .unwrap_or("")
            .trim_start_matches("q=")
            .to_lowercase();
        let hits: Vec<serde_json::Value> = if q.is_empty() {
            Vec::new()
        } else {
            ctx.pois
                .iter()
                .filter(|p| p.name.to_lowercase().contains(&q))
                .take(20)
                .map(|p| {
                    serde_json::json!({
                        "name": p.name,
                        "kind": p.kind,
                        "x": p.x,
                        "z": p.z,
                        "access_node": p.access_node_hex,
                    })
                })
                .collect()
        };
        let out = serde_json::json!({ "query": q, "results": hits });
        return http_reply(
            stream,
            "200 OK",
            "application/json",
            out.to_string().as_bytes(),
        );
    }

    // GET /api/settings（A2a-M1 §60：会话配置只读）
    if method == "GET" && qpath == "/api/settings" {
        let cfg = nav_router::session::SessionConfig::default();
        let out = serde_json::json!({
            "profile": format!("{:?}", cfg.profile),
            "matcher": {
                "w_distance": cfg.matcher.w_distance,
                "w_heading": cfg.matcher.w_heading,
                "w_topology": cfg.matcher.w_topology,
            },
            "reroute": {
                "confirm_frames": cfg.reroute.confirm_frames,
                "confirm_distance_m": cfg.reroute.confirm_distance_m,
            },
            "arrive_margin_m": cfg.arrive_margin_m,
        });
        return http_reply(
            stream,
            "200 OK",
            "application/json",
            out.to_string().as_bytes(),
        );
    }

    http_reply(stream, "404 Not Found", "text/plain", b"not found")
}

pub fn server_cli(
    dataset_dir: &str,
    trace_path: Option<&str>,
    port: u16,
    web_root: &str,
    fake_signal: bool,
) {
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
    let mut turns_map: std::collections::HashMap<(u64, u32), i8> = std::collections::HashMap::new();
    for j in &junctions.junctions {
        for m in &j.movements {
            turns_map.insert((j.uid, m.id), m.turn_type);
        }
    }
    let turns: TurnLookup = turns_map;
    drop(junctions); // 运行时不需要 junction.graph 数据（~54MB，bench 同口径）

    let shared = ServerShared::new();

    // 数据源线程：回放或实时 → session.on_frame → 广播（Arc 跨线程共享只读图）。
    let g2 = graph.clone();
    let s2 = spatial.clone();
    let thread_graph = graph.clone();
    let thread_spatial = spatial.clone();
    let thread_shared = shared.clone();
    let thread_trace = trace_path.map(|p| p.to_string());
    let turns_src = turns.clone();
    std::thread::spawn(move || {
        // A2a-M4 / P4R Batch 2：`--fake-signal` 由 session 层注入合成灯态剧本，
        // 使 reminders 与结构化 glosa 都经真实 §36/§37/§38 计算，而非事后改写 JSON。
        let cfg = nav_router::session::SessionConfig {
            fake_signal,
            ..nav_router::session::SessionConfig::default()
        };
        // 消费 UI 目的地请求（§60：POST /api/route → 导航启动）——回放/实时共用（A2c-M4）
        let consume_dest = |session: &mut nav_router::session::NavigationSession| {
            let pending = *thread_shared.pending_dest.lock().unwrap();
            if let Some((tx, tz)) = pending {
                *thread_shared.pending_dest.lock().unwrap() = None;
                if let Some(snap_pt) =
                    nav_router::snap::snap_nearest(&thread_graph, &thread_spatial, tx, tz, 300.0)
                {
                    let dest = nav_router::destination::Destination {
                        kind: nav_router::destination::DestKind::Coordinate,
                        name: "目标".to_string(),
                        position: (tx, 0.0, tz),
                        access_snap: snap_pt,
                    };
                    if session.set_destination(dest).is_ok() {
                        eprintln!("[server] 目的地已设置 ({tx:.0},{tz:.0})");
                        // A2a-M2：map_state 事件（§60）——路线 polyline 一次推送（UI 绘制地图层）
                        if let Some(route) = session.route() {
                            let mut polyline: Vec<[f64; 2]> = Vec::new();
                            for &eid in &route.edges {
                                let e = &thread_graph.edges[eid as usize];
                                let pts = thread_graph.edge_geometry(e);
                                for pt in pts {
                                    polyline.push([pt.0, pt.2]);
                                }
                            }
                            let ev = serde_json::json!({
                                "type": "map_state",
                                "distance_m": route.distance_m,
                                "polyline": polyline,
                                "destination": [tx, tz],
                            });
                            let json = ev.to_string();
                            thread_shared.broadcast(&json); // 仅广播（latest_json 保持 vehicle 语义）
                        }
                    }
                }
            }
        };
        let mut session = nav_router::session::NavigationSession::new(
            g2.clone(),
            s2.clone(),
            turns_src.clone(),
            cfg.clone(),
        );
        if let Some(tp) = thread_trace {
            let frames: Vec<nav_telemetry::TraceFrame> =
                nav_telemetry::replay(std::path::Path::new(&tp))
                    .unwrap_or_else(|e| {
                        eprintln!("读取 trace 失败: {e}");
                        std::process::exit(1);
                    })
                    .collect();
            if frames.is_empty() {
                eprintln!("trace 为空");
                std::process::exit(1);
            }
            // 循环回放（UI 演示/验证用——帧间按 sim time 节流）。
            // A2c-M3：每轮重建 session（Arrived 分支为 no-op——不重置则首轮到达后永久卡死）。
            loop {
                let mut last_sim: Option<u64> = None;
                for f in frames.iter() {
                    consume_dest(&mut session);
                    if let Some(ls) = last_sim {
                        let dt = f.snap.simulation_time.saturating_sub(ls);
                        if dt > 0 {
                            std::thread::sleep(std::time::Duration::from_micros(dt.min(200_000)));
                        }
                    }
                    last_sim = Some(f.snap.simulation_time);
                    let snap = session.on_frame(&f.snap);
                    let json = snapshot_json(&snap);
                    *thread_shared.latest_json.lock().unwrap() = json.clone();
                    thread_shared.broadcast(&json);
                }
                // 重建 session（新导航周期）
                session = nav_router::session::NavigationSession::new(
                    g2.clone(),
                    s2.clone(),
                    turns_src.clone(),
                    cfg.clone(),
                );
                eprintln!("[server] 回放循环重启（session 已重置）");
            }
        } else {
            let mut src = nav_telemetry::TelemetrySource::new(std::time::Duration::from_secs(2));
            loop {
                consume_dest(&mut session);
                match src.poll() {
                    nav_telemetry::TelemetryState::Fresh(snap) => {
                        let snap2 = session.on_frame(&snap);
                        let json = snapshot_json(&snap2);
                        *thread_shared.latest_json.lock().unwrap() = json.clone();
                        thread_shared.broadcast(&json);
                    }
                    nav_telemetry::TelemetryState::Stale => {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    nav_telemetry::TelemetryState::Disconnected => {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                }
            }
        }
    });

    // HTTP listener（accept + 每连接线程；graph 经 gate 锁访问）
    let listener = TcpListener::bind(("0.0.0.0", port)).unwrap_or_else(|e| {
        eprintln!("绑定端口 {port} 失败: {e}");
        std::process::exit(1);
    });
    let pois = nav_dataset::load_pois(&std::path::Path::new(dataset_dir).join("search.db"))
        .unwrap_or_default();
    let ctx = Arc::new(ServerCtx {
        graph,
        spatial,
        dataset_dir: dataset_dir.to_string(),
        shared,
        pois,
    });
    let web_root = web_root.to_string();
    eprintln!("[server] listening on :{port}（web root: {web_root}）");
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let ctx = ctx.clone();
        let web_root = web_root.clone();
        std::thread::spawn(move || {
            let _ = handle_conn(&mut stream, &ctx, &web_root);
        });
    }
}

fn handle_conn(
    stream: &mut TcpStream,
    ctx: &Arc<ServerCtx>,
    web_root: &str,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    let (req_line, headers, body_len, mut body_rem) = read_http_head(stream)?;

    // WebSocket 升级
    if req_line.starts_with("GET /ws") {
        let upgrade = headers
            .iter()
            .find(|(k, _)| k == "upgrade")
            .map(|(_, v)| v.to_lowercase() == "websocket")
            .unwrap_or(false);
        let key = headers
            .iter()
            .find(|(k, _)| k == "sec-websocket-key")
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        if upgrade && !key.is_empty() {
            let accept = ws_accept_key(&key);
            let head = format!(
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
            );
            stream.write_all(head.as_bytes())?;
            stream.flush()?;
            ctx.shared
                .ws_clients
                .lock()
                .unwrap()
                .push(stream.try_clone()?);
            while let Ok((opcode, _)) = ws_recv_frame(stream) {
                if opcode == 0x8 {
                    break; // close
                }
                if opcode == 0x9 {
                    let _ = ws_send_frame(stream, 0xA, b""); // pong
                }
            }
            return Ok(());
        }
    }

    // HTTP 静态文件（web_root 映射；路径穿越防护）
    let path = req_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
    if path.starts_with("/api/") {
        // body = 同包剩余 + 补读（Content-Length 语义）
        if body_rem.len() < body_len {
            let mut rest = vec![0u8; body_len - body_rem.len()];
            stream.read_exact(&mut rest)?;
            body_rem.extend_from_slice(&rest);
        }
        return handle_http(
            ctx,
            stream,
            &req_line,
            &body_rem[..body_len.min(body_rem.len())],
        );
    }
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let mut full = std::path::PathBuf::from(web_root);
    for comp in std::path::Path::new(rel).components() {
        if let std::path::Component::Normal(c) = comp {
            full.push(c);
        } else {
            return http_reply(stream, "403 Forbidden", "text/plain", b"forbidden");
        }
    }
    match std::fs::read(&full) {
        Ok(body) => {
            // P4R-02：PMTiles 客户端依赖 Byte Serving（Range/206）读取档案头与目录。
            let range = match headers.iter().find(|(k, _)| k == "range") {
                Some((_, v)) => parse_range(v, body.len() as u64),
                None => RangeRequest::Ignore,
            };
            let head_only = req_line.starts_with("HEAD ");
            http_reply_static(stream, content_type(rel), &body, range, head_only)
        }
        Err(_) => http_reply(stream, "404 Not Found", "text/plain", b"not found"),
    }
}
