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
        "reminders": reminders,
        "destination": s.destination,
        "diagnostics": s.diagnostics,
    })
    .to_string()
}

// ─── HTTP 工具 ───────────────────────────────────────────────────────────────
pub(crate) fn http_reply(
    w: &mut dyn Write,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
        body.len()
    );
    w.write_all(head.as_bytes())?;
    w.write_all(body)?;
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
}
