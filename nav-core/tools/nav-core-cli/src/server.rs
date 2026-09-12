//! P4 nav-server 核心（§60/§61）：RFC6455 最小 WebSocket + HTTP 工具。
//! 零第三方依赖：SHA1 手写 RFC3174；服务端发送无掩码帧；接收解掩码。
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;

use nav_router::session::NavigationSnapshot;

// ─── RFC3174 SHA-1（WS 握手需要；测试向量见 tests）──────────────────────────
pub(crate) fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    let mut w = [0u32; 80];
    for chunk in msg.chunks(64) {
        for i in 0..80 {
            w[i] = if i < 16 {
                u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap())
            } else {
                (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1)
            };
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, hi) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&hi.to_be_bytes());
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

// ─── RFC6455 最小 WebSocket ─────────────────────────────────────────────────
pub(crate) fn ws_accept_key(key: &str) -> String {
    let mut input = key.trim().to_string();
    input.push_str("258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    base64(&sha1(input.as_bytes()))
}

/// 服务端发送帧（无掩码；payload ≤ 125 与 16-bit 长度两种形态）。
pub(crate) fn ws_send_frame(w: &mut dyn Write, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut hdr = vec![0x80 | opcode];
    if payload.len() < 126 {
        hdr.push(payload.len() as u8);
    } else {
        hdr.push(126);
        hdr.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    w.write_all(&hdr)?;
    w.write_all(payload)?;
    w.flush()
}

/// 接收一帧（客户端掩码帧）：返回 (opcode, payload)。
pub(crate) fn ws_recv_frame(r: &mut dyn Read) -> std::io::Result<(u8, Vec<u8>)> {
    let mut hdr = [0u8; 2];
    r.read_exact(&mut hdr)?;
    let _fin = hdr[0] & 0x80 != 0;
    let opcode = hdr[0] & 0x0F;
    let masked = hdr[1] & 0x80 != 0;
    let mut len = (hdr[1] & 0x7F) as usize;
    if len == 126 {
        let mut b = [0u8; 2];
        r.read_exact(&mut b)?;
        len = u16::from_be_bytes(b) as usize;
    } else if len == 127 {
        let mut b = [0u8; 8];
        r.read_exact(&mut b)?;
        len = u64::from_be_bytes(b) as usize;
    }
    let mut mask = [0u8; 4];
    if masked {
        r.read_exact(&mut mask)?;
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    if masked {
        for (i, p) in payload.iter_mut().enumerate() {
            *p ^= mask[i % 4];
        }
    }
    Ok((opcode, payload))
}

// ─── 共享会话状态 ────────────────────────────────────────────────────────────
pub struct ServerShared {
    /// 最新快照 JSON（数据源线程写，HTTP/WS 读）。
    pub latest_json: Mutex<String>,
    /// 已连接的 WS 客户端（广播线程轮询；连接线程向此注册）。
    pub ws_clients: Mutex<Vec<TcpStream>>,
    /// 待设目的地（HTTP /api/route 写入，数据源线程消费——UI 设目的地 → session 导航）。
    pub pending_dest: Mutex<Option<(f64, f64)>>,
}

impl ServerShared {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(ServerShared {
            latest_json: Mutex::new("null".to_string()),
            ws_clients: Mutex::new(Vec::new()),
            pending_dest: Mutex::new(None),
        })
    }

    /// 广播一帧 JSON 给所有 WS 客户端（写超时/失败即断开移除——A2c-M6：
    /// 慢客户端不得冻结数据源管道——每连接写超时 2s）。
    pub fn broadcast(&self, json: &str) {
        let mut clients = self.ws_clients.lock().unwrap();
        let mut dead = Vec::new();
        for (i, c) in clients.iter_mut().enumerate() {
            let _ = c.set_write_timeout(Some(std::time::Duration::from_secs(2)));
            if ws_send_frame(c, 0x1, json.as_bytes()).is_err() {
                dead.push(i);
            }
        }
        for &i in dead.iter().rev() {
            clients.remove(i);
        }
    }
}

/// NavigationSnapshot → §60 vehicle 事件 JSON（手写映射；无 serde derive）。
pub(crate) fn snapshot_json(s: &NavigationSnapshot) -> String {
    let state = match s.state {
        nav_router::session::SessionState::Idle => "idle",
        nav_router::session::SessionState::DestinationSet => "destination_set",
        nav_router::session::SessionState::Planning => "planning",
        nav_router::session::SessionState::Navigating => "navigating",
        nav_router::session::SessionState::SuspectedOffRoute => "suspected_off_route",
        nav_router::session::SessionState::Rerouting => "rerouting",
        nav_router::session::SessionState::Arrived => "arrived",
        nav_router::session::SessionState::Paused => "paused",
        nav_router::session::SessionState::LostPosition => "lost_position",
        nav_router::session::SessionState::Error => "error",
    };
    let maneuver = s.next_maneuver.as_ref().map(|m| {
        serde_json::json!({
            "type": format!("{:?}", m.mtype),
            "distance_m": m.distance_from_prev,
            "bearing_deg": m.bearing_change.to_degrees(),
            "roundabout_exit": m.roundabout_exit,
        })
    });
    let signal = s.upcoming_signal.as_ref().map(|up| {
        serde_json::json!({
            "state": up.state.map(|st| format!("{:?}", st)),
            "remaining_s": up.remaining_time,
            "confidence": format!("{:?}", up.confidence),
        })
    });
    // §38 GLOSA 结构化投影（P4R Batch 2）：直接暴露 GlosaAdvice 的量化结果，
    // 不做二次换算，UI 也不得从 reminder.text 反推。
    //   有建议 → {"min_kmh": i16, "max_kmh": i16}
    //   无建议 → null（字段恒存在，语义明确，避免 UI 用 undefined 猜）
    // feasible 不单独输出：字段存在本身即表示区间可行，且 min ≤ max、min ≥ 0
    // 由 glosa_advice 的量化保证（见 reminder.rs 单测），故不作为独立字段暴露。
    let glosa = s
        .glosa
        .map(|g| serde_json::json!({ "min_kmh": g.v_min_kmh, "max_kmh": g.v_max_kmh }));
    // A2a-M2：warning 结构化——kind（ReminderEvent 变体）+ severity（1=warning/0=info）
    let reminders: Vec<serde_json::Value> = s
        .reminders
        .iter()
        .map(|r| {
            let (kind, severity) = match r {
                nav_router::speak::ReminderEvent::SpeedLimitChange { .. } => {
                    ("speed_limit_change", 0)
                }
                nav_router::speak::ReminderEvent::OverSpeed { .. } => ("overspeed", 1),
                nav_router::speak::ReminderEvent::RedLight { .. } => ("red_light", 1),
                nav_router::speak::ReminderEvent::GreenImminent => ("green_imminent", 0),
                nav_router::speak::ReminderEvent::Glosa { .. } => ("glosa", 0),
            };
            serde_json::json!({
                "kind": kind,
                "severity": severity,
                "text": nav_router::speak::to_speech_zh(r),
            })
        })
        .collect();
    let (px, _, pz) = s.position.unwrap_or((0.0, 0.0, 0.0));
    serde_json::json!({
        "type": "vehicle",
        "state": state,
        "position": [px, pz],
        "speed_kmh": s.speed_kmh,
        "map_limit_kmh": s.map_limit_kmh,
        "remaining_m": s.remaining_m,
        "remaining_s": s.remaining_s,
        "progress": s.progress,
        "matched_edge": s.matched_edge,
        "next_maneuver": maneuver,
        "upcoming_signal": signal,
        "glosa": glosa,
        "reminders": reminders,
        "destination": s.destination,
        "diagnostics": s.diagnostics,
    })
    .to_string()
}

// ─── HTTP 工具 ───────────────────────────────────────────────────────────────

/// 已判定的 CORS 响应头（由 `security::cors_headers_for` / `preflight_headers_for`
/// 产出）。空表 ⇒ 不发出任何 CORS 头，这正是「非白名单 origin 不得获得许可」的
/// 落地形态。P4R Batch 3 之前此处无条件写 `Access-Control-Allow-Origin: *`。
pub(crate) type CorsHeaders = Vec<(&'static str, String)>;

/// 把任意头序列化为响应头片段（每个头一行，含 CRLF）。
///
/// 键类型固定为 `&str`；值泛化到 `Display` 以便同时接受编译期常量表
/// （`(&str, &str)`）与运行时构造的 CORS 头（`(&str, String)`）。
///
/// 注意：写出的值只来自调用方提供的常量表或 `cors_headers_for`（后者写出的也是
/// 白名单常量本身），因此响应头结构不可能被请求内容改变——CRLF 注入在构造层面
/// 就不可达。
pub(crate) fn header_block<V: std::fmt::Display>(headers: &[(&str, V)]) -> String {
    let mut s = String::new();
    for (k, v) in headers {
        s.push_str(k);
        s.push_str(": ");
        s.push_str(&v.to_string());
        s.push_str("\r\n");
    }
    s
}

/// 把已判定的 CORS 头序列化为响应头片段（每个头一行，含 CRLF）。
pub(crate) fn cors_block(cors: &[(&'static str, String)]) -> String {
    header_block(cors)
}

/// 写一个完整 HTTP 响应，`extra` 为附加响应头。
///
/// 所有响应都带 `security::HARDENING_HEADERS`（nosniff / no-referrer）；
/// 动态 API 的 `Cache-Control: no-store` 由调用方经 `extra` 传入，因为静态资源
/// 仍需正常缓存（vendor JS 与 PMTiles 的 Range 请求依赖缓存语义）。
pub(crate) fn http_reply_full(
    w: &mut dyn Write,
    status: &str,
    content_type: &str,
    body: &[u8],
    cors: &[(&'static str, String)],
    extra: &[(&str, String)],
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{}{}{}Connection: close\r\n\r\n",
        body.len(),
        header_block(crate::security::HARDENING_HEADERS),
        header_block(extra),
        cors_block(cors)
    );
    w.write_all(head.as_bytes())?;
    w.write_all(body)?;
    w.flush()
}

pub(crate) fn http_reply(
    w: &mut dyn Write,
    status: &str,
    content_type: &str,
    body: &[u8],
    cors: &[(&'static str, String)],
) -> std::io::Result<()> {
    http_reply_full(w, status, content_type, body, cors, &[])
}

/// 动态 API 响应的 `extra`：实时导航状态与令牌都不得进入任何缓存。
pub(crate) fn no_store() -> Vec<(&'static str, String)> {
    crate::security::NO_STORE_HEADERS
        .iter()
        .map(|(k, v)| (*k, (*v).to_string()))
        .collect()
}

/// 有界排空入站字节，返回排空字节数。
///
/// 用途单一：在**刻意不解析请求**的早期拒绝路径上，接收队列里仍留有客户端发来的
/// 字节；Windows 在仍有未读入站数据时关闭连接会发 RST，可能把刚写出的拒绝响应一并
/// 丢弃，使客户端只看到「连接被中止」而非 403（实测：同一地址三次连接中出现一次
/// WinError 10053）。排空只丢弃字节、不做任何解析，因此不改变「拒绝路径不解释请求
/// 内容」这一性质。
///
/// 必须有界：否则对端可以持续发送数据把拒绝线程钉住。
pub(crate) fn drain_inbound(r: &mut dyn Read, max_bytes: usize) -> usize {
    let mut buf = [0u8; 2048];
    let mut total = 0usize;
    while total < max_bytes {
        match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(_) => break, // 超时/复位即停止：排空是尽力而为，不承担正确性
        }
    }
    total
}

/// `/api/metadata` 的非敏感投影（纯函数，独立于 socket 以便直接断言隐私性质）。
///
/// 原实现返回 `ctx.dataset_dir` 的**完整绝对路径**（其中含构建机的盘符与目录名），
/// 会把开发机用户名与目录结构泄露给任何能访问该端点的对端。此处只暴露数据集
/// 的**目录名**（`data/europe-v5` → `europe-v5`）与图规模统计。`dataset_dir`
/// 在进程内部照常保留，用于加载与日志。
pub(crate) fn metadata_json(nodes: usize, edges: usize, dataset_dir: &str) -> String {
    serde_json::json!({
        "nodes": nodes,
        "edges": edges,
        "dataset": dataset_display_name(dataset_dir),
    })
    .to_string()
}

/// 数据集显示名：取路径最后一段，绝不返回完整路径。
pub(crate) fn dataset_display_name(dataset_dir: &str) -> String {
    std::path::Path::new(dataset_dir)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// `Range` 头解析结果。
///
/// 需要区分三种情形，不能把「语法非法」与「语法合法但越界」混为一谈：
/// 前者按 RFC 9110 §14.2 忽略 Range 返回 200 全量；后者 RFC 9110 §15.5.17
/// 要求 416 + `Content-Range: bytes */<len>`。PMTiles 客户端依赖 416 分支在
/// 档案小于首个 16384 字节探测窗口时回退到正确长度。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RangeRequest {
    /// 无 Range 头、多区间、语法非法 —— 按无 Range 处理。
    Ignore,
    /// 单区间且可满足，闭区间 (start, end)。
    Satisfiable(u64, u64),
    /// 语法合法但起点越界。
    Unsatisfiable,
}

/// 解析单区间 `Range` 头：`bytes=start-end` / `bytes=start-` / `bytes=-suffix`。
pub(crate) fn parse_range(value: &str, total: u64) -> RangeRequest {
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return RangeRequest::Ignore;
    };
    if spec.contains(',') {
        return RangeRequest::Ignore; // 多区间不支持
    }
    let Some((a, b)) = spec.trim().split_once('-') else {
        return RangeRequest::Ignore;
    };
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() {
        // 后缀区间：最后 N 字节
        let Ok(n) = b.parse::<u64>() else {
            return RangeRequest::Ignore;
        };
        if n == 0 || total == 0 {
            return RangeRequest::Ignore;
        }
        return RangeRequest::Satisfiable(total.saturating_sub(n), total - 1);
    }
    let Ok(start) = a.parse::<u64>() else {
        return RangeRequest::Ignore;
    };
    let end = if b.is_empty() {
        total.saturating_sub(1)
    } else {
        match b.parse::<u64>() {
            Ok(e) => e,
            Err(_) => return RangeRequest::Ignore,
        }
    };
    if start >= total {
        return RangeRequest::Unsatisfiable;
    }
    if start > end {
        return RangeRequest::Ignore; // 语法合法但语义无效（last < first）：按无 Range 处理
    }
    RangeRequest::Satisfiable(start, end)
}

/// 静态文件响应（P4R-02）：PMTiles 客户端依赖 HTTP Byte Serving 读取档案头与目录，
/// 因此静态路径必须支持 Range → 206 Partial Content 并声明 Accept-Ranges。
/// 无 Range 时返回 200 全量；HEAD 返回与 GET 一致的头部但不带 body。
pub(crate) fn http_reply_static(
    w: &mut dyn Write,
    content_type: &str,
    body: &[u8],
    range: RangeRequest,
    head_only: bool,
    cors: &[(&'static str, String)],
) -> std::io::Result<()> {
    let total = body.len() as u64;
    let (status, len, slice, extra) = match range {
        RangeRequest::Satisfiable(s, e) => {
            let e = e.min(total - 1);
            (
                "206 Partial Content",
                e - s + 1,
                Some((s as usize, e as usize)),
                format!("Content-Range: bytes {s}-{e}/{total}\r\n"),
            )
        }
        // 起点越界：416 并给出当前表示长度（RFC 9110 §15.5.17）
        RangeRequest::Unsatisfiable => (
            "416 Range Not Satisfiable",
            0,
            None,
            format!("Content-Range: bytes */{total}\r\n"),
        ),
        RangeRequest::Ignore => ("200 OK", total, None, String::new()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\n{extra}{}Connection: close\r\n{}\r\n",
        header_block(crate::security::HARDENING_HEADERS),
        cors_block(cors)
    );
    w.write_all(head.as_bytes())?;
    if !head_only {
        match slice {
            Some((s, e)) => w.write_all(&body[s..=e])?,
            None if status.starts_with("200") => w.write_all(body)?,
            None => {}
        }
    }
    w.flush()
}

/// 解析请求行与 headers（body 长度由 Content-Length 给出）。
/// 返回 (请求行, headers, content-length, 已读入的 body 剩余字节)——head 与 body
/// 同包到达时 body 不得丢失（否则 read_exact 阻塞至超时）。
pub(crate) type HttpHead = (String, Vec<(String, String)>, usize, Vec<u8>);

pub(crate) fn read_http_head(r: &mut dyn Read) -> std::io::Result<HttpHead> {
    const CRLFCRLF: [u8; 4] = [13, 10, 13, 10];
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 1024];
    let head_end = loop {
        let n = r.read(&mut tmp)?;
        if n == 0 {
            break buf.len(); // EOF：客户端关闭
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == CRLFCRLF) {
            break pos + 4;
        }
        if buf.len() > 65536 {
            break buf.len();
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end.min(buf.len())]);
    let mut lines = head.split("\r\n");
    let req_line = lines.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            headers.push((k.trim().to_lowercase(), v.trim().to_string()));
        }
    }
    let clen = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .map(|(_, v)| v.parse::<usize>().unwrap_or(0))
        .unwrap_or(0);
    let remaining = buf[head_end.min(buf.len())..].to_vec();
    Ok((req_line, headers, clen, remaining))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_vectors() {
        // RFC3174 测试向量
        assert_eq!(
            sha1(b"abc"),
            [
                0xA9, 0x99, 0x3E, 0x36, 0x47, 0x06, 0x81, 0x6A, 0xBA, 0x3E, 0x25, 0x71, 0x78, 0x50,
                0xC2, 0x6C, 0x9C, 0xD0, 0xD8, 0x9D
            ]
        );
        assert_eq!(
            sha1(b"The quick brown fox jumps over the lazy dog"),
            [
                0x2F, 0xD4, 0xE1, 0xC6, 0x7A, 0x2D, 0x28, 0xFC, 0xED, 0x84, 0x9E, 0xE1, 0xBB, 0x76,
                0xE7, 0x39, 0x1B, 0x93, 0xEB, 0x12
            ]
        );
    }

    #[test]
    fn ws_accept_key_known() {
        // RFC6455 示例：Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==
        // → s3pPLMBiTxaQ9kYGzzhZRbK+xOo=
        assert_eq!(
            ws_accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn read_http_head_same_packet() {
        // head+body 同包到达：remaining 必须保留 body（回归：曾因真实 CR 字节字面量
        // 导致 \r\n\r\n 永不匹配——body 丢失、read_exact 阻塞至 EOF）
        let req = b"POST /api/route HTTP/1.1\r\nHost: localhost\r\nContent-Length: 43\r\n\r\n{\"from\":[-58456,32832],\"to\":[-52925,36510]}";
        let mut r = &req[..];
        let (req_line, headers, clen, rem) = read_http_head(&mut r).unwrap();
        assert!(req_line.starts_with("POST /api/route"));
        assert_eq!(clen, 43);
        assert_eq!(rem.len(), 43, "同包 body 必须保留");
        assert_eq!(
            headers
                .iter()
                .find(|(k, _)| k == "content-length")
                .map(|(_, v)| v.as_str()),
            Some("43")
        );
    }

    // ── P4R-02：静态文件 Byte Serving（PMTiles 接入前提）────────────────────

    #[test]
    fn parse_range_forms() {
        use RangeRequest::*;
        assert_eq!(parse_range("bytes=0-99", 1000), Satisfiable(0, 99));
        assert_eq!(parse_range("bytes=100-", 1000), Satisfiable(100, 999));
        assert_eq!(parse_range("bytes=-200", 1000), Satisfiable(800, 999));
        // 端点超出表示长度：由 http_reply_static 截断，解析层保留原值
        assert_eq!(parse_range("bytes=0-99999", 1000), Satisfiable(0, 99999));
        // 语法非法/不支持：忽略 Range（200 全量），不得误判为 416
        assert_eq!(parse_range("bytes=0-10,20-30", 1000), Ignore);
        assert_eq!(parse_range("items=0-10", 1000), Ignore);
        assert_eq!(parse_range("bytes=500-100", 1000), Ignore);
        assert_eq!(parse_range("bytes=abc-def", 1000), Ignore);
        assert_eq!(parse_range("bytes=-0", 1000), Ignore);
        // 语法合法但起点越界：必须 416（PMTiles 客户端据此回退到真实长度）
        assert_eq!(parse_range("bytes=1000-", 1000), Unsatisfiable);
        assert_eq!(parse_range("bytes=5000-6000", 1000), Unsatisfiable);
    }

    #[test]
    fn http_reply_static_range_semantics() {
        use RangeRequest::*;
        let body: Vec<u8> = (0u8..=255).collect();
        let no_cors: &[(&'static str, String)] = &[];

        let mut out = Vec::new();
        http_reply_static(
            &mut out,
            "application/octet-stream",
            &body,
            Satisfiable(10, 19),
            false,
            no_cors,
        )
        .unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(text.starts_with("HTTP/1.1 206 Partial Content\r\n"));
        assert!(text.contains("Content-Range: bytes 10-19/256\r\n"));
        assert!(text.contains("Content-Length: 10\r\n"));
        assert!(text.contains("Accept-Ranges: bytes\r\n"));
        // 206 只回请求区间，不得回全量（比较尾部字节；不可按 \n 切分——payload 含 0x0A）
        assert_eq!(
            &out[out.len() - 10..],
            &body[10..=19],
            "206 必须只回请求区间，不得回全量"
        );
        assert!(out.len() < body.len(), "206 响应不得包含全量表示");

        // 端点越界：截断到表示末尾（RFC 9110 §14.1.2）
        let mut out = Vec::new();
        http_reply_static(
            &mut out,
            "text/plain",
            &body,
            Satisfiable(250, 9999),
            false,
            no_cors,
        )
        .unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(text.contains("Content-Range: bytes 250-255/256\r\n"));
        assert!(text.contains("Content-Length: 6\r\n"));

        // 无 Range：200 全量
        let mut out = Vec::new();
        http_reply_static(&mut out, "text/plain", b"abc", Ignore, false, no_cors).unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 3\r\n"));

        // HEAD：与 GET 相同头部但不带 body
        let mut out = Vec::new();
        http_reply_static(&mut out, "text/plain", b"abc", Ignore, true, no_cors).unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 3\r\n"));
        assert!(
            out.ends_with(b"\r\n\r\n"),
            "HEAD 响应必须在头部结束后终止，不得写 body"
        );

        // HEAD + Range：头部按 206 描述，仍不得写 body
        let mut out = Vec::new();
        http_reply_static(
            &mut out,
            "text/plain",
            &body,
            Satisfiable(0, 99),
            true,
            no_cors,
        )
        .unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(text.starts_with("HTTP/1.1 206 Partial Content\r\n"));
        assert!(text.contains("Content-Length: 100\r\n"));
        assert!(out.ends_with(b"\r\n\r\n"), "HEAD 不得写 body");

        // 起点越界：416 + 表示长度，且不带 body
        let mut out = Vec::new();
        http_reply_static(&mut out, "text/plain", &body, Unsatisfiable, false, no_cors).unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(text.starts_with("HTTP/1.1 416 Range Not Satisfiable\r\n"));
        assert!(text.contains("Content-Range: bytes */256\r\n"));
        assert!(text.contains("Content-Length: 0\r\n"));
        assert!(out.ends_with(b"\r\n\r\n"));
    }

    // ── P4R Batch 3：CORS 头注入形态 ────────────────────────────────────────

    #[test]
    fn http_reply_has_no_cors_header_by_default() {
        let mut out = Vec::new();
        http_reply(&mut out, "200 OK", "text/plain", b"ok", &[]).unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(
            !text.contains("Access-Control-Allow-Origin"),
            "无许可判定时不得发出任何 CORS 头（P4R Batch 3 移除了通配符）"
        );
        assert!(!text.contains('*'));
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(out.ends_with(b"ok"));
    }

    #[test]
    fn http_reply_emits_specific_origin_and_vary() {
        let cors = crate::security::cors_headers_for(Some("http://tauri.localhost"));
        let mut out = Vec::new();
        http_reply(&mut out, "200 OK", "application/json", b"{}", &cors).unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(text.contains("Access-Control-Allow-Origin: http://tauri.localhost\r\n"));
        assert!(text.contains("Vary: Origin\r\n"));
        assert!(
            !text.contains("Access-Control-Allow-Origin: *"),
            "绝不得回通配符"
        );
        // 头部必须以空行结束，body 原样跟随
        assert!(text.contains("\r\n\r\n{}"), "头部与 body 之间必须有空行");
    }

    #[test]
    fn http_reply_denied_origin_adds_nothing() {
        let cors = crate::security::cors_headers_for(Some("http://evil.example"));
        let mut out = Vec::new();
        http_reply(
            &mut out,
            "401 Unauthorized",
            "application/json",
            b"{}",
            &cors,
        )
        .unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(!text.contains("Access-Control"));
        assert!(!text.contains("Vary"));
    }

    #[test]
    fn static_reply_carries_cors_only_for_allowed_origin() {
        let cors = crate::security::cors_headers_for(Some("http://tauri.localhost"));
        let mut out = Vec::new();
        http_reply_static(
            &mut out,
            "text/plain",
            b"x",
            RangeRequest::Ignore,
            false,
            &cors,
        )
        .unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(text.contains("Access-Control-Allow-Origin: http://tauri.localhost\r\n"));
        assert!(text.contains("Content-Length: 1\r\n"));
        assert!(!text.contains('*'));

        let mut out = Vec::new();
        http_reply_static(
            &mut out,
            "text/plain",
            b"x",
            RangeRequest::Ignore,
            false,
            &[],
        )
        .unwrap();
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(!text.contains("Access-Control"));
        assert!(!text.contains('*'));
    }

    // ── P4R Batch 3：/api/metadata 路径泄露（S7）────────────────────────────

    #[test]
    fn drain_inbound_is_bounded_and_stops_at_eof() {
        // 全部读走
        let data = [7u8; 100];
        let mut r = &data[..];
        assert_eq!(drain_inbound(&mut r, 8192), 100);
        // 上限生效：数据远多于上限时不得无限读
        let big = vec![1u8; 100_000];
        let mut r = &big[..];
        let n = drain_inbound(&mut r, 4096);
        assert!(
            (4096..=4096 + 2048).contains(&n),
            "必须在上限附近停止，实际 {n}"
        );
        // 空输入立即返回
        let empty: &[u8] = &[];
        let mut r = empty;
        assert_eq!(drain_inbound(&mut r, 8192), 0);
    }

    #[test]
    fn early_reject_response_is_delivered_before_drain() {
        // 顺序契约：必须先写拒绝响应再排空，否则排空会先吃掉请求、响应仍可能被 RST 丢弃。
        // 此处以「写响应 + 排空」组合验证总字节数，锁定两者不互相吞并。
        let mut out = Vec::new();
        http_reply(&mut out, "403 Forbidden", "text/plain", b"forbidden", &[]).unwrap();
        let written = out.len();
        assert!(String::from_utf8_lossy(&out).starts_with("HTTP/1.1 403 Forbidden\r\n"));
        let req = b"GET /api/snapshot HTTP/1.1\r\nHost: x\r\n\r\n";
        let mut r = &req[..];
        let drained = drain_inbound(&mut r, 8192);
        assert_eq!(drained, req.len());
        assert_eq!(out.len(), written, "排空不得改动已写出的响应");
    }

    #[test]
    fn metadata_json_never_leaks_absolute_path() {
        // 这些是**合成示例路径**，不是本机真实路径：本测试断言输出中不含输入路径，
        // 用任意路径均可成立，因此不得把开发机的真实用户名/工程路径写进仓库。
        for dir in [
            r"E:\work\ets2nav\data\europe-v5",
            r"C:\Users\example\ets2nav\data\europe-v5",
            "/home/example/ets2nav/data/europe-v5",
            r"data\europe-v5",
        ] {
            let s = metadata_json(123, 456, dir);
            assert!(!s.contains(dir), "不得回显完整 dataset_dir: {dir}");
            assert!(!s.contains(r":\"), "不得含 Windows 盘符路径: {s}");
            assert!(!s.contains("/"), "不得含路径分隔符: {s}");
            assert!(!s.contains('\\'), "不得含反斜杠: {s}");
            assert!(!s.to_lowercase().contains("users"), "不得含用户名段: {s}");
            assert!(
                !s.to_lowercase().contains("projects"),
                "不得含工程路径段: {s}"
            );
            let v: serde_json::Value = serde_json::from_str(&s).unwrap();
            assert_eq!(v["dataset"], "europe-v5");
            assert_eq!(v["nodes"], 123);
            assert_eq!(v["edges"], 456);
        }
    }

    #[test]
    fn dataset_display_name_is_last_segment() {
        assert_eq!(dataset_display_name(r"E:\a\b\europe-v5"), "europe-v5");
        assert_eq!(dataset_display_name("data/europe-v5"), "europe-v5");
        assert_eq!(dataset_display_name("data/europe-v5/"), "europe-v5");
        assert_eq!(dataset_display_name("europe-v5"), "europe-v5");
        assert_eq!(dataset_display_name("."), "unknown");
        assert_eq!(dataset_display_name(""), "unknown");
        // 关键：任何输入都不得让输出等于输入的完整路径
        for d in [r"C:\Users\x\d", "/tmp/x/d"] {
            assert_ne!(dataset_display_name(d), d);
            assert!(!dataset_display_name(d).contains([':', '\\', '/']));
        }
    }

    // ── P4R-2：GLOSA 数据契约（snapshot_json 的 UI-facing projection）──────────

    /// Vehicle 帧 JSON 的最小构造（只填契约断言涉及的字段）。
    fn snapshot_fixture() -> NavigationSnapshot {
        use nav_router::session::SessionState;
        NavigationSnapshot {
            state: SessionState::Navigating,
            position: Some((1.0, 0.0, 2.0)),
            matched_edge: Some(7),
            match_confidence: None,
            route_distance_m: Some(1000.0),
            remaining_m: Some(400.0),
            remaining_s: Some(50.0),
            progress: Some(0.6),
            next_maneuver: None,
            upcoming_signal: None,
            glosa: None,
            reminders: Vec::new(),
            speed_kmh: 43.2,
            map_limit_kmh: 50,
            destination: Some("test".to_string()),
            diagnostics: String::new(),
        }
    }

    fn glosa_advice(min: i16, max: i16) -> nav_router::reminder::GlosaAdvice {
        nav_router::reminder::GlosaAdvice {
            v_min_kmh: min,
            v_max_kmh: max,
            feasible: true,
        }
    }

    #[test]
    fn snapshot_json_glosa_present_is_structured() {
        let mut s = snapshot_fixture();
        s.glosa = Some(glosa_advice(50, 60));
        let v: serde_json::Value = serde_json::from_str(&snapshot_json(&s)).unwrap();
        assert_eq!(v["glosa"]["min_kmh"], 50);
        assert_eq!(v["glosa"]["max_kmh"], 60);
        assert_eq!(
            v["glosa"]["min_kmh"].as_i64(),
            Some(50),
            "必须是 JSON 数值，不是字符串"
        );
        assert_eq!(v["type"], "vehicle");
    }

    #[test]
    fn snapshot_json_glosa_single_value_preserved() {
        // min == max（单值建议）不得被序列化折叠或改写
        let mut s = snapshot_fixture();
        s.glosa = Some(glosa_advice(50, 50));
        let v: serde_json::Value = serde_json::from_str(&snapshot_json(&s)).unwrap();
        assert_eq!(v["glosa"]["min_kmh"], 50);
        assert_eq!(v["glosa"]["max_kmh"], 50);
    }

    #[test]
    fn snapshot_json_glosa_absent_is_null_field() {
        // 契约：无建议时字段存在且为 null（不是缺失、不是 0/0——0/0 会被 UI 当作
        // 「建议 0 km/h」，正是必须避免的降级歧义）
        let s = snapshot_fixture();
        assert!(s.glosa.is_none());
        let v: serde_json::Value = serde_json::from_str(&snapshot_json(&s)).unwrap();
        assert!(v.get("glosa").is_some(), "glosa 字段必须恒存在");
        assert!(v["glosa"].is_null(), "无建议必须是 null，不得为 0/0 或缺失");
    }

    #[test]
    fn snapshot_json_reminders_preserved_with_glosa() {
        // §4 兼容性：结构化 glosa 不得取代或破坏既有 reminders / TTS 通道
        use nav_router::speak::ReminderEvent;
        let mut s = snapshot_fixture();
        s.glosa = Some(glosa_advice(50, 60));
        s.reminders = vec![
            ReminderEvent::Glosa {
                v_min_kmh: 50,
                v_max_kmh: 60,
            },
            ReminderEvent::OverSpeed { limit_kmh: 50 },
        ];
        let v: serde_json::Value = serde_json::from_str(&snapshot_json(&s)).unwrap();
        let rs = v["reminders"].as_array().unwrap();
        assert_eq!(rs.len(), 2, "既有 reminders 不得因新增字段而丢失");
        assert_eq!(rs[0]["kind"], "glosa");
        assert_eq!(rs[0]["severity"], 0);
        assert_eq!(rs[0]["text"], "建议保持50到60", "TTS 文本通道保持原样");
        assert_eq!(rs[1]["kind"], "overspeed");
        assert_eq!(rs[1]["severity"], 1);
    }

    #[test]
    fn snapshot_json_glosa_survives_speech_gate_suppression() {
        // 本条锁定 P4R Batch 2 的核心缺陷形态：§48 ReminderGate 每 30s 才放行一次
        // glosa 语音事件，而 UI 卡片需要连续显示建议区间。因此存在大量「glosa 有值
        // 但 reminders 中没有 glosa 事件」的帧——结构化字段必须在这种帧上照常输出，
        // 否则 UI 会在两次播报之间显示空白或残留旧值。
        let mut s = snapshot_fixture();
        s.glosa = Some(glosa_advice(45, 55));
        s.reminders = Vec::new(); // 播报门限抑制：本帧无任何语音事件
        let v: serde_json::Value = serde_json::from_str(&snapshot_json(&s)).unwrap();
        assert_eq!(v["reminders"].as_array().unwrap().len(), 0);
        assert_eq!(
            v["glosa"]["min_kmh"], 45,
            "结构化建议不得随语音门限一起消失"
        );
        assert_eq!(v["glosa"]["max_kmh"], 55);
    }

    #[test]
    fn snapshot_json_glosa_never_emits_non_finite_or_negative() {
        // 全值域扫描：i16 不可能产生 NaN/Infinity；且 glosa_advice 的量化保证
        // min ≥ 0、min ≤ max。此处锁定「JSON 层不得引入负数/浮点化」。
        for (min, max) in [(0i16, 5i16), (5, 5), (0, 0), (i16::MAX, i16::MAX)] {
            let mut s = snapshot_fixture();
            s.glosa = Some(glosa_advice(min, max));
            let v: serde_json::Value = serde_json::from_str(&snapshot_json(&s)).unwrap();
            let lo = v["glosa"]["min_kmh"].as_i64().unwrap();
            let hi = v["glosa"]["max_kmh"].as_i64().unwrap();
            assert!(lo >= 0 && hi >= 0, "不得输出负速度: {lo}..{hi}");
            assert!(lo <= hi, "下限不得超过上限: {lo}..{hi}");
            let raw = snapshot_json(&s);
            assert!(!raw.contains("NaN"), "JSON 不得含 NaN");
            assert!(!raw.contains("Infinity"), "JSON 不得含 Infinity");
        }
    }
}
