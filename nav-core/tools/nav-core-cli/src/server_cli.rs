use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use crate::server::{
    http_reply, read_http_head, snapshot_json, ws_accept_key, ws_recv_frame, ws_send_frame,
    ServerShared,
};
use nav_router::maneuver::TurnLookup;

// ─── server_cli：HTTP + WS + 数据源线程 ─────────────────────────────────────

pub struct ServerCtx {
    pub graph: std::sync::Arc<nav_graph::CompactGraph>,
    pub spatial: std::sync::Arc<nav_spatial::SpatialIndex>,
    pub dataset_dir: String,
    pub shared: Arc<ServerShared>,
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

    http_reply(stream, "404 Not Found", "text/plain", b"not found")
}

pub fn server_cli(dataset_dir: &str, trace_path: Option<&str>, port: u16, web_root: &str) {
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
        let cfg = nav_router::session::SessionConfig::default();
        let mut session = nav_router::session::NavigationSession::new(g2, s2, turns_src, cfg);
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
            // 循环回放（UI 演示/验证用——帧间按 sim time 节流）
            loop {
                let mut last_sim: Option<u64> = None;
                for f in &frames {
                    // 消费 UI 目的地请求（§60：POST /api/route → 导航启动）
                    if let Some((tx, tz)) = *thread_shared.pending_dest.lock().unwrap() {
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
                            }
                        }
                    }
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
                eprintln!("[server] 回放循环重启");
            }
        } else {
            let mut src = nav_telemetry::TelemetrySource::new(std::time::Duration::from_secs(2));
            loop {
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
    let ctx = Arc::new(ServerCtx {
        graph,
        spatial,
        dataset_dir: dataset_dir.to_string(),
        shared,
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
        Ok(body) => http_reply(stream, "200 OK", content_type(rel), &body),
        Err(_) => http_reply(stream, "404 Not Found", "text/plain", b"not found"),
    }
}
