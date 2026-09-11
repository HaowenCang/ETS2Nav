use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use crate::security::{
    self, request_path, AuthOutcome, ExposureMode, LanCandidate, PeerClass, SessionToken,
};
use crate::server::{
    http_reply, http_reply_static, metadata_json, parse_range, read_http_head, snapshot_json,
    ws_accept_key, ws_recv_frame, ws_send_frame, CorsHeaders, RangeRequest, ServerShared,
};
use nav_router::maneuver::TurnLookup;

// ─── server_cli：HTTP + WS + 数据源线程 ─────────────────────────────────────

/// 服务器启动参数。
///
/// 用具名结构体而非位置参数：先前 `--port` 因按位置读取而被静默忽略过一次
/// （P4R Batch 2），参数增多后位置参数只会放大同类风险。
pub struct ServerOptions {
    pub dataset_dir: String,
    pub trace_path: Option<String>,
    pub port: u16,
    pub web_root: String,
    pub fake_signal: bool,
    /// 显式 `--lan`：监听 0.0.0.0 并要求私网对端携带会话令牌。
    pub lan: bool,
}

/// 安全上下文：暴露模式、会话令牌、局域网候选地址。
pub struct SecurityCtx {
    pub mode: ExposureMode,
    /// 仅 `Lan` 模式下为 `Some`。回环模式不生成令牌。
    pub token: Option<SessionToken>,
    /// 启动时枚举一次。运行中网卡增减不刷新——二维码是启动期快照，
    /// 服务端对端类别判定始终基于连接的**实际**来源地址，与候选表无关。
    pub candidates: Vec<LanCandidate>,
    pub port: u16,
}

pub struct ServerCtx {
    pub graph: std::sync::Arc<nav_graph::CompactGraph>,
    pub spatial: std::sync::Arc<nav_spatial::SpatialIndex>,
    pub dataset_dir: String,
    pub shared: Arc<ServerShared>,
    /// POI 表（/api/search 用；启动时从 search.db 加载——内存过滤）。
    pub pois: Vec<nav_dataset::PoiRecord>,
    pub security: SecurityCtx,
}

impl ServerCtx {
    /// 请求来源的鉴权判定（薄封装，判定逻辑在 `security::authorize`）。
    fn decide(&self, peer: PeerClass, presented: Option<&str>, path: &str) -> AuthOutcome {
        if !security::is_protected_api(path) {
            return AuthOutcome::Allowed;
        }
        security::authorize(
            self.security.mode,
            peer,
            self.security.token.as_ref(),
            presented,
        )
    }

    fn token(&self) -> Option<&SessionToken> {
        self.security.token.as_ref()
    }
}

/// 鉴权失败的统一响应（401/403），并附带该请求 origin 对应的 CORS 头。
fn deny(stream: &mut TcpStream, outcome: AuthOutcome, cors: &CorsHeaders) -> std::io::Result<()> {
    match outcome {
        AuthOutcome::Unauthorized => http_reply(
            stream,
            "401 Unauthorized",
            "application/json",
            b"{\"error\":\"unauthorized\"}",
            cors,
        ),
        _ => http_reply(
            stream,
            "403 Forbidden",
            "application/json",
            b"{\"error\":\"forbidden\"}",
            cors,
        ),
    }
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
///
/// P4R Batch 3 的请求处理顺序（顺序本身是安全语义的一部分）：
///   1. 解析请求行/headers；
///   2. `OPTIONS` 预检 → 仅对白名单 origin 返回许可头；
///   3. `/api/lan-bootstrap` → 回环限定（唯一令牌出口），先于通用鉴权；
///   4. `/api/*` → 默认拒绝式鉴权（缺失/错误令牌 401）；
///   5. 业务路由；
///   6. 静态文件（不鉴权，见 `security::is_protected_api` 的威胁模型说明）。
fn handle_http(
    ctx: &ServerCtx,
    stream: &mut TcpStream,
    peer: PeerClass,
    req_line: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> std::io::Result<()> {
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let target = parts.next().unwrap_or("/");
    let path = request_path(target);

    // Origin 只用于决定「是否发出 CORS 许可头」，绝不参与授权判定（§15）：
    // 非浏览器客户端可以伪造任意 Origin，CORS 也不是访问控制机制。
    let origin = headers
        .iter()
        .find(|(k, _)| k == "origin")
        .map(|(_, v)| v.as_str());
    let cors = security::cors_headers_for(origin);

    // ── CORS 预检 ──────────────────────────────────────────────────────────
    if method == "OPTIONS" {
        let pf = security::preflight_headers_for(origin);
        if pf.is_empty() {
            // 非白名单 origin：不得返回任何许可头（含 Allow-Methods/Headers）
            return http_reply(stream, "403 Forbidden", "text/plain", b"forbidden", &[]);
        }
        return http_reply(stream, "204 No Content", "text/plain", b"", &pf);
    }

    // ── 引导端点：唯一的令牌出口，且只对回环对端开放 ────────────────────────
    if path == "/api/lan-bootstrap" {
        if method != "GET" {
            return http_reply(
                stream,
                "405 Method Not Allowed",
                "application/json",
                b"{\"error\":\"method not allowed\"}",
                &cors,
            );
        }
        let (code, json) = security::lan_bootstrap_response(
            ctx.security.mode,
            peer,
            ctx.security.port,
            ctx.security.token.as_ref(),
            &ctx.security.candidates,
        );
        let status = if code == 200 {
            "200 OK"
        } else {
            "403 Forbidden"
        };
        return http_reply(stream, status, "application/json", json.as_bytes(), &cors);
    }

    // ── 动态 API 鉴权：默认拒绝 ────────────────────────────────────────────
    // HTTP API 只接受 `Authorization: Bearer`，**不**接受 `?token=`
    // （query 会进入访问日志/历史/Referer/诊断）。该性质由集成测试显式锁定。
    let presented = security::bearer_token(headers);
    let outcome = ctx.decide(peer, presented, path);
    if outcome != AuthOutcome::Allowed {
        return deny(stream, outcome, &cors);
    }

    // POST /api/route：{"from":[x,z],"to":[x,z]}
    if method == "POST" && path == "/api/route" {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
            return http_reply(
                stream,
                "400 Bad Request",
                "application/json",
                b"{\"error\":\"bad json\"}",
                &cors,
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
                &cors,
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
                &cors,
            );
        };
        let Some(s2) = nav_router::snap::snap_nearest(graph, spatial, tx, tz, 300.0) else {
            return http_reply(
                stream,
                "404 Not Found",
                "application/json",
                b"{\"error\":\"dest not routable\"}",
                &cors,
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
                // 副作用严格发生在鉴权之后：未授权请求不得触达此行（S4）。
                *ctx.shared.pending_dest.lock().unwrap() = Some((tx, tz));
                http_reply(
                    stream,
                    "200 OK",
                    "application/json",
                    out.to_string().as_bytes(),
                    &cors,
                )
            }
            None => http_reply(
                stream,
                "404 Not Found",
                "application/json",
                b"{\"error\":\"no route\"}",
                &cors,
            ),
        };
    }

    // GET /api/snapshot：轮询备用通道
    if method == "GET" && path == "/api/snapshot" {
        let latest = ctx.shared.latest_json.lock().unwrap().clone();
        return http_reply(
            stream,
            "200 OK",
            "application/json",
            latest.as_bytes(),
            &cors,
        );
    }

    // GET /api/metadata：只暴露非敏感元信息（不得回显绝对路径）
    if method == "GET" && path == "/api/metadata" {
        let out = metadata_json(
            ctx.graph.node_count(),
            ctx.graph.edges.len(),
            &ctx.dataset_dir,
        );
        return http_reply(stream, "200 OK", "application/json", out.as_bytes(), &cors);
    }

    // GET /api/search?q=xxx（A2a-M1 §60：POI 搜索——search.db 全量加载后内存过滤）
    if method == "GET" && path == "/api/search" {
        let q = security::query_param(target, "q")
            .unwrap_or_default()
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
            &cors,
        );
    }

    // GET /api/settings（A2a-M1 §60：会话配置只读）
    if method == "GET" && path == "/api/settings" {
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
            &cors,
        );
    }

    http_reply(stream, "404 Not Found", "text/plain", b"not found", &cors)
}

pub fn server_cli(opts: &ServerOptions) {
    let ServerOptions {
        dataset_dir,
        trace_path,
        port,
        web_root,
        fake_signal,
        lan,
    } = opts;
    let (port, fake_signal, lan) = (*port, *fake_signal, *lan);
    let mode = if lan {
        ExposureMode::Lan
    } else {
        ExposureMode::LoopbackOnly
    };
    // LAN 模式下令牌生成失败必须**拒绝启动**：绝不能降级为「无令牌的 LAN 服务」。
    let token = if mode == ExposureMode::Lan {
        match SessionToken::generate() {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("[server] 会话令牌生成失败，拒绝以 --lan 启动: {e}");
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    let candidates = if mode == ExposureMode::Lan {
        security::discover_lan_candidates()
    } else {
        Vec::new()
    };

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
    let thread_trace = trace_path.clone();
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
    // P4R Batch 3：默认只绑回环；`--lan` 才绑 0.0.0.0，且来源由 classify_peer 限制。
    let listener = TcpListener::bind(mode.bind_addr(port)).unwrap_or_else(|e| {
        eprintln!("绑定端口 {port} 失败: {e}");
        std::process::exit(1);
    });
    let actual_port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let pois = nav_dataset::load_pois(&std::path::Path::new(dataset_dir).join("search.db"))
        .unwrap_or_default();
    let candidate_count = candidates.len();
    let ctx = Arc::new(ServerCtx {
        graph,
        spatial,
        dataset_dir: dataset_dir.to_string(),
        shared,
        pois,
        security: SecurityCtx {
            mode,
            token,
            candidates,
            port: actual_port,
        },
    });
    let web_root = web_root.to_string();
    // 启动横幅刻意**不含**令牌本体：stdout/stderr 会进入终端回滚、日志与
    // CI artifact。令牌只经回环 `/api/lan-bootstrap` 交付。
    eprintln!(
        "[server] listening on {}:{}（web root: {web_root}，LAN 访问: {}）",
        mode.bind_addr(port).ip(),
        actual_port,
        if lan {
            format!("启用（需会话令牌；本机候选地址 {candidate_count} 个）")
        } else {
            "禁用（仅回环）".to_string()
        }
    );
    if lan && candidate_count == 0 {
        eprintln!("[server] 未发现 RFC1918 私网地址——二维码将不可用（服务仍可经回环使用）");
    }
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let ctx = ctx.clone();
        let web_root = web_root.clone();
        // 来源判定用**实际**对端地址，不用任何请求内容或启动期候选表。
        let peer = stream
            .peer_addr()
            .map(|a| security::classify_peer(a.ip()))
            .unwrap_or(PeerClass::Disallowed);
        std::thread::spawn(move || {
            let _ = handle_conn(&mut stream, &ctx, &web_root, peer);
        });
    }
}

fn handle_conn(
    stream: &mut TcpStream,
    ctx: &Arc<ServerCtx>,
    web_root: &str,
    peer: PeerClass,
) -> std::io::Result<()> {
    // 非回环也非私网的来源：在解析任何请求内容之前拒绝。不读请求行/headers，
    // 因此未授权来源无法让服务端解析其构造的输入。
    if peer == PeerClass::Disallowed {
        let _ = http_reply(
            stream,
            "403 Forbidden",
            "text/plain",
            b"forbidden: source address not permitted",
            &[],
        );
        // 写响应后再有界排空并不解析地丢弃入站字节：否则关闭连接时 Windows 会因
        // 存在未读数据而发 RST，把刚写出的 403 一并丢掉（实测复现 WinError 10053），
        // 使客户端只看到「连接中止」而非明确的拒绝状态。
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(250)));
        crate::server::drain_inbound(stream, 8192);
        return Ok(());
    }

    stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    let (req_line, headers, body_len, mut body_rem) = read_http_head(stream)?;

    let origin = headers
        .iter()
        .find(|(k, _)| k == "origin")
        .map(|(_, v)| v.as_str());
    let cors = security::cors_headers_for(origin);

    // WebSocket 升级
    let target = req_line.split_whitespace().nth(1).unwrap_or("/");
    if request_path(target) == "/ws" {
        // ── 鉴权必须先于 101 ──────────────────────────────────────────────
        // 浏览器 WebSocket API 无法设置 Authorization 头，故 WS 令牌经 query 传递；
        // 同时接受 Authorization 头（便于原生客户端）。**HTTP API 不接受 query
        // 令牌**——该差异由集成测试锁定。
        let presented = security::bearer_token(&headers)
            .map(|s| s.to_string())
            .or_else(|| security::query_param(target, "token"));
        let outcome =
            security::authorize(ctx.security.mode, peer, ctx.token(), presented.as_deref());
        if outcome != AuthOutcome::Allowed {
            // 未完成 upgrade，不发送 101、不注册到广播列表。
            return deny(stream, outcome, &cors);
        }

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
                "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n{}\r\n",
                crate::server::cors_block(&cors)
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
                    // RFC 6455 §5.5.1：收到 Close 且此前未发送过 Close 时**必须**回送
                    // Close 帧。原实现只 break 不回送，对端因此停留在 CLOSING 状态，
                    // `close` 事件迟迟不触发——浏览器主动 close 时表现为「socket 关不掉、
                    // 状态栏不变、也不重连」（E2E-LAN-05 实测 readyState 卡在 2）。
                    // 服务端被 kill 属于 TCP 中止，会立即触发 onclose，故该缺陷不会由
                    // 「杀掉服务器再重启」这类用例暴露。
                    let _ = ws_send_frame(stream, 0x8, &[]);
                    // 回送后用 shutdown 明确结束写方向，使对端立即看到 EOF 而不必等待
                    // 广播线程下一次写失败才回收（ws_clients 持有一份 socket 克隆，
                    // 仅 drop 本函数内的 stream 并不会关闭连接）。
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                    break;
                }
                if opcode == 0x9 {
                    let _ = ws_send_frame(stream, 0xA, b""); // pong
                }
            }
            return Ok(());
        }
        return http_reply(
            stream,
            "400 Bad Request",
            "text/plain",
            b"bad upgrade",
            &cors,
        );
    }

    // HTTP 静态文件（web_root 映射；路径穿越防护）
    let path = request_path(target).to_string();
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
            peer,
            &req_line,
            &headers,
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
            return http_reply(stream, "403 Forbidden", "text/plain", b"forbidden", &cors);
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
            http_reply_static(stream, content_type(rel), &body, range, head_only, &cors)
        }
        Err(_) => http_reply(stream, "404 Not Found", "text/plain", b"not found", &cors),
    }
}
