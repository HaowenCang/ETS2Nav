//! P4R Batch 3 / 3.5：暴露面控制、会话令牌、来源（Origin/Host）策略与 CORS 判定核心。
//!
//! 结构原则：**所有安全判定都是纯函数**（`classify_peer` / `authorize` /
//! `ServerAuthorities::*` / `bootstrap_response` / `cors_headers_for` /
//! `rank_candidates`），socket 与 HTTP 处理层只负责调用它们并把结果落成状态码。
//! 这样安全边界可以被穷举式单元测试覆盖，而不必依赖启动真实服务器；真实 socket
//! 行为另由集成套件覆盖。
//!
//! # 两层互不替代的机制（Batch 3.5 的核心修正）
//!
//! Batch 3 把 `PeerClass::Loopback` 当作「可信」，于是回环对端对所有动态 API 与
//! `/ws` 免令牌。该模型混淆了两件不同的事：
//!
//! - **TCP 对端地址**说明「连接由本机的某个进程建立」；
//! - **请求意图**说明「发起者是不是用户认可的那个客户端」。
//!
//! 二者并不等价：远程恶意页面可以让**用户的浏览器**向 `127.0.0.1` 发出请求，
//! 此时服务端看到的对端地址同样是回环。Batch 3.5 以真实 Chromium 复现了两条
//! 具体路径——跨源 simple POST（`Content-Type: text/plain`，无 preflight）改写
//! 导航目的地，以及跨站 WebSocket 读取车辆帧流。
//!
//! 修正后的模型：
//!
//! | 机制 | 决定什么 | 依据 |
//! |------|----------|------|
//! | `classify_peer` | 是否允许建立连接、能否取得令牌 | TCP 对端地址 |
//! | `SessionToken` | 是否允许调用动态 API / `/ws` | 256-bit 随机凭据 |
//! | `ServerAuthorities` | 浏览器来源（Origin/Host）是否属于本服务器 | 监听端口 + 本机实际地址 |
//!
//! 因此：**回环 ≠ 已认证**。身份由令牌承担，对端地址只承担暴露面控制与
//! bootstrap 资格判定。
//!
//! 威胁模型（本轮范围，见 validation report）：同一局域网内**未授权的第三方**，
//! 以及**用户浏览器被诱导访问的恶意页面**，读取实时导航状态（位置/目的地/路线）
//! 或改写导航目的地。不覆盖：公网暴露、本机其他用户进程、TLS/中间人、供应链。
//! 因此不做「公网服务器」级设计。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// 会话令牌的随机字节数（256-bit 安全强度）。
pub const TOKEN_BYTES: usize = 32;
/// 令牌的十六进制字符数。选用 hex 而非 base64：URL query / fragment 无需 percent
/// 编码即可安全携带，测试断言也无需处理转义。
pub const TOKEN_HEX_LEN: usize = TOKEN_BYTES * 2;

// ─── 来源分类 ────────────────────────────────────────────────────────────────

/// 连接来源类别。分类只依赖对端 IP，不依赖任何请求内容——请求内容由未授权方
/// 完全控制，不能作为安全判定的输入。
///
/// **本类别不是身份，也不表示已认证**（Batch 3.5 修正）：它与令牌是两层不同的
/// 机制。`Loopback` 只说明连接由本机某进程建立，浏览器同样可以代表远程页面建立
/// 这种连接（见模块级说明）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerClass {
    /// 127.0.0.0/8。本机 Browser/Tauri 路径；**仍需令牌**，但只有它可以取得令牌。
    Loopback,
    /// RFC1918 私网地址。仅 `--lan` 模式下可达，且必须携带令牌。
    PrivateLan,
    /// 既非回环也非私网（含链路本地 169.254/16、CGNAT 100.64/10、公网地址）。
    Disallowed,
}

/// 对端 IP → 来源类别。
///
/// IPv4 判定按 RFC1918 逐段显式匹配，不依赖 `Ipv4Addr::is_private()` 的实现细节；
/// 二者一致性由单元测试交叉校验（`classify_matches_std_is_private`）。
///
/// IPv6 说明：当前 listener 只绑定 IPv4（`0.0.0.0` / `127.0.0.1`），纯 IPv6 对端
/// 在传输层就不可能连入，故一律判为 `Disallowed`——这是**保守**结果，不是
/// 「已支持 IPv6」。IPv4-mapped 地址（`::ffff:a.b.c.d`）按内层 IPv4 分类，
/// 以便将来监听双栈时行为不退化为「全部拒绝」。IPv6 支持列为 future work。
pub fn classify_peer(ip: IpAddr) -> PeerClass {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => classify_v4(v4),
            None => PeerClass::Disallowed,
        },
    }
}

fn classify_v4(ip: Ipv4Addr) -> PeerClass {
    let [a, b, ..] = ip.octets();
    // 127.0.0.0/8：先于私网判定（127/8 不属于 RFC1918，顺序不影响结果，但语义更清楚）
    if a == 127 {
        return PeerClass::Loopback;
    }
    match (a, b) {
        (10, _) => PeerClass::PrivateLan,        // 10.0.0.0/8
        (172, 16..=31) => PeerClass::PrivateLan, // 172.16.0.0/12
        (192, 168) => PeerClass::PrivateLan,     // 192.168.0.0/16
        _ => PeerClass::Disallowed,
    }
}

// ─── 暴露模式 ────────────────────────────────────────────────────────────────

/// 监听暴露面。默认 `LoopbackOnly`；只有显式 `--lan` 才进入 `Lan`。
///
/// **两种模式都生成会话令牌**（Batch 3.5 修正）：模式只决定连接可达范围，
/// 不决定是否需要认证。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExposureMode {
    /// 只绑定 127.0.0.1。局域网不可达；本机页面经 bootstrap 取得令牌后使用。
    LoopbackOnly,
    /// 绑定 0.0.0.0。回环与私网对端都必须携带令牌；令牌只能从回环取得。
    Lan,
}

impl ExposureMode {
    /// 监听地址。`Lan` 仍绑 `0.0.0.0` 是 bind 的实现需要（无法只绑「私网地址集合」，
    /// 且网卡地址会变化）；来源限制由 `classify_peer` + `authorize` 在连接层执行，
    /// 不依赖 bind 地址。
    pub fn bind_addr(self, port: u16) -> SocketAddr {
        match self {
            ExposureMode::LoopbackOnly => SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            ExposureMode::Lan => SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
        }
    }
}

// ─── 鉴权判定 ────────────────────────────────────────────────────────────────

/// `/api/*` 与 `/ws` 的鉴权结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthOutcome {
    Allowed,
    /// 需要令牌：缺失或错误。对应 401。
    Unauthorized,
    /// 来源类别本身不被接受。对应 403。
    Forbidden,
}

/// 鉴权判定（纯函数）。
///
/// 规则表（Batch 3.5 修正后）：
/// | 模式         | Loopback            | PrivateLan          | Disallowed |
/// |--------------|---------------------|---------------------|------------|
/// | LoopbackOnly | 令牌正确→Allowed    | Forbidden           | Forbidden  |
/// | Lan          | 令牌正确→Allowed    | 令牌正确→Allowed    | Forbidden  |
///
/// 即：**对端类别只决定可达性，令牌决定授权**。回环不再是免令牌理由——
/// 浏览器可以被远程页面驱使去连 `127.0.0.1`，此时对端同样显示为 `Loopback`，
/// 而请求意图完全不可信（Batch 3.5 以真实 Chromium 复现，见模块级说明）。
///
/// `LoopbackOnly` 下私网对端仍判 `Forbidden` 而非 `Unauthorized`：默认模式下
/// 私网对端根本不该连进来（bind 层已挡住），判 `Forbidden` 是纵深防御，且语义
/// 正确——它不是「缺令牌」，而是「这个来源不该出现」。
///
/// 自举问题如何解决：本机页面同样拿不到令牌，因此需要一个**唯一**且仅对回环
/// 开放的令牌出口 `/api/bootstrap`。它是 `is_protected_api` 的唯一豁免，且该豁免
/// 由来源类别与浏览器 Origin 双重限制，不构成「回环免认证」的一般化通道。
pub fn authorize(
    mode: ExposureMode,
    peer: PeerClass,
    expected: &SessionToken,
    presented: Option<&str>,
) -> AuthOutcome {
    if peer == PeerClass::Disallowed {
        return AuthOutcome::Forbidden;
    }
    if mode == ExposureMode::LoopbackOnly && peer == PeerClass::PrivateLan {
        return AuthOutcome::Forbidden;
    }
    match presented {
        Some(got) if expected.verify(got) => AuthOutcome::Allowed,
        _ => AuthOutcome::Unauthorized,
    }
}

/// 定长字节的常量时间相等比较。
///
/// 长度不等时立即返回：令牌长度恒为 64 且是公开信息，不构成泄露。逐字节累积
/// 差异后统一判定，避免按首个不同字节提前返回而泄露前缀匹配长度。
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ─── 会话令牌 ────────────────────────────────────────────────────────────────

/// 进程级会话令牌。每次进程启动重新生成，不持久化、不落盘、不写日志。
#[derive(Clone)]
pub struct SessionToken(String);

impl SessionToken {
    /// 用 OS CSPRNG 生成 256-bit 令牌。
    ///
    /// 失败即返回错误：调用方必须据此**拒绝启动**——绝不能降级为「无令牌的
    /// LAN 服务」或任何自制伪随机回退。
    pub fn generate() -> std::io::Result<Self> {
        let mut raw = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut raw)
            .map_err(|e| std::io::Error::other(format!("OS CSPRNG (getrandom) 失败: {e}")))?;
        Ok(SessionToken(hex_lower(&raw)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 定长常量时间比较（用于对端提交的令牌）。
    ///
    /// 长度先判：令牌长度恒为 `TOKEN_HEX_LEN` 且是公开信息（出现在二维码与
    /// 前端校验里），提前返回不泄露任何秘密，同时避免对畸形输入做无谓比较。
    pub fn verify(&self, presented: &str) -> bool {
        if presented.len() != TOKEN_HEX_LEN {
            return false;
        }
        ct_eq(self.0.as_bytes(), presented.as_bytes())
    }
}

impl std::fmt::Debug for SessionToken {
    /// 刻意不实现为派生 Debug：令牌绝不能因 `{:?}` 意外进入日志或 panic 信息。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}

// ─── 凭据提取 ────────────────────────────────────────────────────────────────

/// 从已解析的 headers 中取 `Authorization: Bearer <token>`。
///
/// HTTP API **只**接受该通道：query 参数会进入访问日志、浏览器历史、Referer
/// 与诊断输出，不适合承载长期凭据。
pub fn bearer_token(headers: &[(String, String)]) -> Option<&str> {
    let v = headers
        .iter()
        .find(|(k, _)| k == "authorization")
        .map(|(_, v)| v.as_str())?;
    let (scheme, rest) = v.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let tok = rest.trim();
    if tok.is_empty() {
        None
    } else {
        Some(tok)
    }
}

/// 从请求目标中取 query 参数（仅 `/ws` 使用）。
///
/// 不做 percent 解码：令牌是十六进制，属 RFC3986 unreserved 字符集，无需转义；
/// 不做解码即不存在「解码后与比较值不一致」的歧义面。
pub fn query_param(target: &str, name: &str) -> Option<String> {
    let q = target.split_once('?')?.1;
    for pair in q.split('&') {
        let (k, v) = match pair.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        if k == name {
            return Some(v.to_string());
        }
    }
    None
}

/// 请求目标去掉 query 后的路径。
pub fn request_path(target: &str) -> &str {
    target.split('?').next().unwrap_or("/")
}

/// 取某个请求头（已小写键）的值。
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

// ─── 服务器 authority 与浏览器来源策略 ───────────────────────────────────────

/// 本服务器可被访问的 `host:port` 集合。
///
/// 用途有两处，且**都不把请求自报的 `Host` 当作信任来源**：
/// 1. `Host` 头校验（§14）：`Host: evil.example` + `Origin: http://evil.example`
///    不得因为「二者一致」而被当作同源放行——`evil.example` 不在本表内，
///    请求直接被拒；
/// 2. 浏览器 `Origin` 校验（§13）：同源判定只查本表，**不**与 `Host` 比对。
///
/// 表中的 host 项来自服务器自身状态：回环名（`127.0.0.1`/`localhost`/`::1`）
/// 与启动时枚举到的本机 RFC1918 候选地址；端口是 listener 的实际端口。
/// 因此不存在「任意 Host + 任意 Origin」的自洽放行路径。
#[derive(Debug, Clone)]
pub struct ServerAuthorities {
    port: u16,
    hosts: Vec<String>,
}

/// 未带端口时假定的端口（HTTP 默认端口）。
const DEFAULT_HTTP_PORT: u16 = 80;

impl ServerAuthorities {
    /// 依据实际监听端口与本机候选地址构造。
    pub fn new(port: u16, candidates: &[LanCandidate]) -> Self {
        let mut hosts = vec![
            "127.0.0.1".to_string(),
            "localhost".to_string(),
            "::1".to_string(),
        ];
        for c in candidates {
            let s = c.address.to_string();
            if !hosts.contains(&s) {
                hosts.push(s);
            }
        }
        ServerAuthorities { port, hosts }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// 该 host 是否属于本机（已归一化：小写、去方括号）。
    pub fn host_known(&self, host: &str) -> bool {
        self.hosts.iter().any(|h| h == host)
    }

    /// `Host` 头是否可接受。
    ///
    /// 刻意**严格**：端口必须等于实际监听端口（未写端口时按 HTTP 默认端口 80
    /// 处理）。这使下列请求被拒：
    /// - 攻击者控制的域名（DNS rebinding：域名解析到 127.0.0.1，但 `Host` 是
    ///   攻击者的域名）；
    /// - 端口不符的 authority。
    ///
    /// 代价（已记入报告）：以本机主机名/组播名（`mypc.local`）访问不再被接受，
    /// 必须使用回环名或二维码/设置面板给出的实际 IP。URL 是由服务端生成并交付
    /// 的，该限制不影响正常流程。
    pub fn host_header_allowed(&self, raw: &str) -> bool {
        match split_authority(raw) {
            Some((host, port)) => port == self.port && self.host_known(&host),
            None => false,
        }
    }

    /// 浏览器 `Origin` 是否指向本服务器（同源判定）。
    ///
    /// 只接受 `http://` scheme：服务端不提供 TLS，把 `https://` 视为本服务器会
    /// 制造一个不存在的信任面。
    pub fn origin_is_self(&self, origin: &str) -> bool {
        let Some(rest) = origin.strip_prefix("http://") else {
            return false;
        };
        match split_authority(rest) {
            Some((host, port)) => port == self.port && self.host_known(&host),
            None => false,
        }
    }
}

/// 拆分 `host[:port]`，返回归一化 host（小写、去 `[]`）与端口。
///
/// 未写端口时取 `DEFAULT_HTTP_PORT`。端口段必须为纯数字且非空——`127.0.0.1:8a`
/// 这类畸形输入返回 `None` 而不是被截断成合法值。
fn split_authority(raw: &str) -> Option<(String, u16)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (host_raw, port) = if let Some(rest) = raw.strip_prefix('[') {
        // IPv6 字面量：[::1]:8123 / [::1]
        let (h, tail) = rest.split_once(']')?;
        let port = match tail.strip_prefix(':') {
            Some(p) => parse_port(p)?,
            None if tail.is_empty() => DEFAULT_HTTP_PORT,
            None => return None,
        };
        (h, port)
    } else {
        match raw.split_once(':') {
            Some((h, p)) => (h, parse_port(p)?),
            None => (raw, DEFAULT_HTTP_PORT),
        }
    };
    let host = host_raw.trim().to_ascii_lowercase();
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    Some((host, port))
}

fn parse_port(s: &str) -> Option<u16> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u16>().ok()
}

/// 浏览器来源是否被接受（`/ws` 握手与 `/api/bootstrap` 共用同一策略）。
///
/// 两类被接受：
/// 1. **本服务器自身的同源页面**——authority 在本机已知地址表内且端口相符；
/// 2. **CORS 白名单**（实测确认的 Tauri origin）。
///
/// `None`（无 `Origin` 头）不在本函数职责内：非浏览器客户端本来就不发 `Origin`，
/// 它必须靠令牌通过认证。调用方必须先判令牌；本函数只是「若浏览器携带 Origin，
/// 则该 Origin 必须合规」这一附加条件。
///
/// `Origin: null`（sandbox iframe / 部分重定向场景）必然落到「不在表内」分支被拒。
pub fn browser_origin_accepted(auth: &ServerAuthorities, origin: &str) -> bool {
    auth.origin_is_self(origin) || cors_allowed_origin(origin).is_some()
}

// ─── 请求体媒体类型策略（CSRF 纵深防御）─────────────────────────────────────

/// 该请求是否必须声明 `Content-Type: application/json`。
///
/// 覆盖 `/api/` 下一切带请求体的方法，而不只是当前的 `POST /api/route`：
/// 按方法而非按端点枚举，使将来新增的写入端点默认继承该约束。
pub fn body_must_be_json(method: &str, path: &str) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH") && path.starts_with("/api/")
}

/// `Content-Type` 是否为 `application/json`（允许 `; charset=utf-8` 等参数）。
///
/// 缺失该头即返回 false——「没声明」与「声明错了」在本策略下同样不可接受。
pub fn is_json_content_type(headers: &[(String, String)]) -> bool {
    match header(headers, "content-type") {
        Some(v) => v
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("application/json"),
        None => false,
    }
}

// ─── 响应硬化头 ──────────────────────────────────────────────────────────────

/// 所有响应都附带的硬化头。
///
/// `nosniff`：阻止浏览器忽略 `Content-Type` 做内容嗅探（例如把 `map.pmtiles`
/// 或 API 的 JSON 当作脚本/HTML 解释）。
/// `no-referrer`：页面 URL 不进入任何出站 `Referer`。当前 URL 不含令牌
/// （fragment 不参与 `Referer`，query 令牌已被服务端拒绝），故此项是纵深防御。
pub const HARDENING_HEADERS: &[(&str, &str)] = &[
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
];

/// 动态 API 响应的缓存指令：实时导航状态与令牌都不得进入任何缓存。
pub const NO_STORE_HEADERS: &[(&str, &str)] = &[("Cache-Control", "no-store")];

// ─── 局域网地址发现 ──────────────────────────────────────────────────────────

/// 一个候选局域网地址。附带网卡名仅用于**让用户在多个候选中做知情选择**，
/// 不参与任何安全判定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanCandidate {
    pub address: Ipv4Addr,
    /// 网卡 FriendlyName（如 `Wi-Fi`、`vEthernet (Default Switch)`）。仅作展示标签。
    pub interface: String,
    /// 网卡 OperStatus 是否为 Up（OS 提供的事实，非名称启发式）。
    pub active: bool,
}

/// 候选排序：active 优先，其次按地址数值升序。
///
/// 刻意**不做**「按网卡类型猜测哪块是 Wi-Fi」的名称启发式——那在
/// Hyper-V/VPN/WSL 并存时会产生一个自信但错误的静默选择，正是本轮要求避免的
/// 行为。排序只决定「哪一个排在最前」，全部候选都会返回给 UI。
/// 排序键完全确定：同一组输入必得同一顺序。
pub fn rank_candidates(mut v: Vec<LanCandidate>) -> Vec<LanCandidate> {
    v.sort_by_key(|c| (!c.active, u32::from(c.address)));
    v
}

/// 枚举本机可用局域网候选地址（RFC1918、非回环、非链路本地）。
///
/// 发现失败返回空列表而非 panic：没有候选只意味着二维码无法生成，UI 会显式
/// 报告「未发现可用局域网地址」，不应让服务启动失败。
pub fn discover_lan_candidates() -> Vec<LanCandidate> {
    let ifaces = match if_addrs::get_if_addrs() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[server] 网卡枚举失败（二维码地址将不可用）: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for i in &ifaces {
        let if_addrs::IfAddr::V4(v4) = &i.addr else {
            continue; // IPv6 未接入（见 classify_peer 说明）
        };
        // 复用 classify_peer 作为唯一判定口径：候选集与「谁算私网对端」必须同源，
        // 否则会出现「二维码给出一个服务端自己会拒绝对端类别」的地址。
        if classify_peer(IpAddr::V4(v4.ip)) != PeerClass::PrivateLan {
            continue;
        }
        out.push(LanCandidate {
            address: v4.ip,
            interface: i.name.clone(),
            active: i.is_oper_up(),
        });
    }
    rank_candidates(out)
}

// ─── 引导端点 ────────────────────────────────────────────────────────────────

/// 会话引导端点路径。**唯一**的动态 API 令牌豁免（见 `is_protected_api`）。
///
/// Batch 3 该端点名为 `/api/lan-bootstrap`，其职责被描述为「LAN 令牌出口」。
/// Batch 3.5 改为 `session bootstrap`：令牌在两种模式下都存在，本机页面
/// （`http://127.0.0.1:…` 与 Tauri 的 `http://tauri.localhost`）都经它取得令牌。
/// 旧名不再保留别名——第二个入口只会多出一条需要独立审计的豁免路径。
pub const BOOTSTRAP_PATH: &str = "/api/bootstrap";

/// `/api/bootstrap` 响应：(HTTP 状态码, JSON 体)。
///
/// 三重限制，缺一不可：
/// 1. **对端必须是回环**（`PeerClass::Loopback`）。远程对端 403 且响应不含任何
///    令牌信息——手机不能从服务端取令牌，只能经二维码 fragment 接收。
/// 2. **若请求携带 `Origin`，该 Origin 必须合规**：只能是本服务器自身的同源页面
///    或 CORS 白名单（实测的 Tauri origin）。这挡住「恶意页面从用户浏览器里读取
///    令牌」——即便对端是回环，`Origin: http://127.0.0.1:<别的端口>` 也会被拒。
/// 3. 响应带 `Cache-Control: no-store`（由调用方经 `NO_STORE_HEADERS` 附加）。
///
/// 无 `Origin` 的请求（`curl`、原生客户端、同源 GET）按「非浏览器客户端」处理：
/// `Origin` 不是认证机制，原生客户端可任意伪造或省略它，因此它只作为浏览器场景
/// 的附加约束，真正的门是「回环对端」这一条。
pub fn bootstrap_response(
    mode: ExposureMode,
    peer: PeerClass,
    origin: Option<&str>,
    auth: &ServerAuthorities,
    token: &SessionToken,
    candidates: &[LanCandidate],
) -> (u16, String) {
    if peer != PeerClass::Loopback {
        return (403, r#"{"error":"bootstrap is loopback-only"}"#.to_string());
    }
    if let Some(o) = origin {
        if !browser_origin_accepted(auth, o) {
            return (403, r#"{"error":"origin not allowed"}"#.to_string());
        }
    }
    let list: Vec<serde_json::Value> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            serde_json::json!({
                "address": c.address.to_string(),
                "interface": c.interface,
                "active": c.active,
                "preferred": i == 0,
            })
        })
        .collect();
    let body = serde_json::json!({
        "lan_enabled": mode == ExposureMode::Lan,
        "port": auth.port(),
        "addresses": list,
        "token": token.as_str(),
    });
    (200, body.to_string())
}

// ─── CORS ────────────────────────────────────────────────────────────────────

/// 允许携带凭据的跨源 origin 白名单。
///
/// 只有 `http://tauri.localhost`：该值是**实测**结果——在 Windows 上运行
/// `desktop/target/debug/ets2nav-desktop.exe`（Tauri 2.11.5，`frontendDist` 指向
/// `tools/ets2nav-web/dist`，无 `devUrl`）并对 nav-server 端口做握手捕获，实测
/// WebView2 发出的握手请求头为 `Origin: http://tauri.localhost`。
///
/// 刻意不加入下列「记忆中的」候选，因为它们在本项目当前配置下**未被验证**：
/// `tauri://localhost`（macOS/Linux 自定义协议形式）、`https://tauri.localhost`、
/// `http://localhost:1420`（Tauri 默认 devUrl——本项目 `tauri.conf.json` 未配置
/// `devUrl`，dev 与 prod 均走自定义协议）。若将来引入 `devUrl` 或跨平台构建，
/// 必须重新实测并把测得值加入本表。
pub const CORS_ALLOWED_ORIGINS: &[&str] = &["http://tauri.localhost"];

/// 命中的白名单常量；未命中返回 `None`。判定与响应头生成共用此函数，
/// 以免出现「判定为允许但不发头」或反之的分叉。
fn cors_allowed_origin(origin: &str) -> Option<&'static str> {
    CORS_ALLOWED_ORIGINS.iter().find(|a| **a == origin).copied()
}

/// 依据请求 `Origin` 生成 CORS 响应头。
///
/// 未在白名单内（含无 Origin、含 `null`）→ 返回空表，即**不发出任何许可头**。
/// 绝不回 `*`：通配符与 `Authorization` 凭据组合会把鉴权结果暴露给任意站点。
///
/// `Vary: Origin` 必须存在：响应头随 Origin 变化，缺少它会被共享缓存串用。
/// 写出的值取自白名单常量本身，而非请求里的字面量。二者在命中时内容相同，
/// 但向前者取值的写法使「响应头中不可能出现请求方构造的字节」成为结构性事实，
/// 无需再论证 CRLF 注入不可行。
pub fn cors_headers_for(origin: Option<&str>) -> Vec<(&'static str, String)> {
    match origin.and_then(cors_allowed_origin) {
        Some(matched) => vec![
            ("Access-Control-Allow-Origin", matched.to_string()),
            ("Vary", "Origin".to_string()),
        ],
        None => Vec::new(),
    }
}

/// `OPTIONS` 预检响应头（仅对白名单 origin 生成）。
///
/// 跨源携带 `Authorization` + `Content-Type: application/json` 必然触发预检，
/// 因此这三项必须齐全；`Allow-Headers` / `Allow-Methods` 逐项列举，不用 `*`。
pub fn preflight_headers_for(origin: Option<&str>) -> Vec<(&'static str, String)> {
    let mut h = cors_headers_for(origin);
    if h.is_empty() {
        return h; // 非白名单 origin：连预检也不得许可
    }
    h.push((
        "Access-Control-Allow-Methods",
        "GET, POST, OPTIONS".to_string(),
    ));
    h.push((
        "Access-Control-Allow-Headers",
        "Authorization, Content-Type".to_string(),
    ));
    h.push(("Access-Control-Max-Age", "600".to_string()));
    h
}

/// 需要令牌保护的请求路径。
///
/// 采用**默认拒绝**：`/api/` 下的一切路径都受保护，唯一豁免是 `/api/bootstrap`。
/// 按前缀拒绝而非按端点枚举，使将来新增的 `/*` 端点默认安全。
///
/// **Batch 3.5 修正**：Batch 3 时回环对端对所有受保护路径免令牌，等于把「TCP
/// 对端是 127.0.0.1」当作身份。现在唯一豁免只剩 bootstrap，且该豁免自身受
/// 「回环对端 + Origin 合规」双重限制。
///
/// 静态资源（`/`、`index.html`、JS/CSS/vendor/字体/`map.pmtiles`/manifest）不在
/// 保护范围内，这是刻意的威胁模型决策：手机必须先取到客户端代码，才能读取 URL
/// fragment 中的令牌并携带它。报告据此**不**声称「LAN 服务全部资源均需认证」，
/// 防护目标是**动态导航状态与控制 API**。
pub fn is_protected_api(path: &str) -> bool {
    path.starts_with("/api/") && path != BOOTSTRAP_PATH
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(s.parse().unwrap())
    }

    // ── S 矩阵的基础：来源分类边界 ─────────────────────────────────────────

    #[test]
    fn classify_boundaries_rfc1918() {
        use PeerClass::*;
        // 10.0.0.0/8
        assert_eq!(classify_peer(v4("10.0.0.0")), PrivateLan);
        assert_eq!(classify_peer(v4("10.0.0.1")), PrivateLan);
        assert_eq!(classify_peer(v4("10.255.255.255")), PrivateLan);
        assert_eq!(
            classify_peer(v4("9.255.255.255")),
            Disallowed,
            "10/8 下界外"
        );
        assert_eq!(classify_peer(v4("11.0.0.0")), Disallowed, "10/8 上界外");

        // 172.16.0.0/12 —— 上下界是最容易写错的一段
        assert_eq!(
            classify_peer(v4("172.15.255.255")),
            Disallowed,
            "172.16/12 下界外"
        );
        assert_eq!(classify_peer(v4("172.16.0.0")), PrivateLan);
        assert_eq!(classify_peer(v4("172.31.255.255")), PrivateLan);
        assert_eq!(
            classify_peer(v4("172.32.0.0")),
            Disallowed,
            "172.16/12 上界外"
        );

        // 192.168.0.0/16
        assert_eq!(classify_peer(v4("192.167.255.255")), Disallowed);
        assert_eq!(classify_peer(v4("192.168.0.1")), PrivateLan);
        assert_eq!(classify_peer(v4("192.168.255.255")), PrivateLan);
        assert_eq!(classify_peer(v4("192.169.0.0")), Disallowed);

        // 回环 127.0.0.0/8（整段，不限于 127.0.0.1）
        assert_eq!(classify_peer(v4("127.0.0.1")), Loopback);
        assert_eq!(classify_peer(v4("127.0.0.2")), Loopback);
        assert_eq!(classify_peer(v4("127.255.255.255")), Loopback);
        assert_eq!(classify_peer(v4("126.255.255.255")), Disallowed);

        // 公网与特殊段
        assert_eq!(classify_peer(v4("8.8.8.8")), Disallowed);
        assert_eq!(classify_peer(v4("1.1.1.1")), Disallowed);
        assert_eq!(
            classify_peer(v4("169.254.1.1")),
            Disallowed,
            "链路本地不属私网"
        );
        assert_eq!(
            classify_peer(v4("100.64.0.1")),
            Disallowed,
            "CGNAT 不属 RFC1918"
        );
        assert_eq!(classify_peer(v4("0.0.0.0")), Disallowed);
        assert_eq!(
            classify_peer(v4("198.18.0.1")),
            Disallowed,
            "benchmark 段不属私网"
        );
    }

    #[test]
    fn classify_matches_std_is_private() {
        // 交叉校验：显式八位组匹配必须与 std 的 RFC1918 判定完全一致，
        // 否则「显式实现」与「惯用实现」会在某个边界上分叉而无人发现。
        for a in [0u8, 9, 10, 11, 100, 126, 127, 128, 169, 172, 192, 198, 255] {
            for b in [0u8, 15, 16, 17, 31, 32, 63, 168, 169, 254, 255] {
                for c in [0u8, 1, 128, 255] {
                    for d in [0u8, 1, 254, 255] {
                        let ip = Ipv4Addr::new(a, b, c, d);
                        let mine = classify_peer(IpAddr::V4(ip)) == PeerClass::PrivateLan;
                        assert_eq!(
                            mine,
                            ip.is_private(),
                            "RFC1918 判定分叉: {ip}（本实现={mine}, std={}）",
                            ip.is_private()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn classify_ipv6_is_conservative_not_claiming_support() {
        // 纯 IPv6 → Disallowed：这不是「支持 IPv6 后拒绝」，而是「未接入」的保守结果
        assert_eq!(
            classify_peer("::1".parse().unwrap()),
            PeerClass::Disallowed,
            "纯 IPv6 未接入，一律判 Disallowed（不得因 ::1 看似回环而放行）"
        );
        assert_eq!(
            classify_peer("fe80::1".parse().unwrap()),
            PeerClass::Disallowed
        );
        // v4-mapped 按内层 IPv4 分类：将来双栈监听时行为不退化
        assert_eq!(
            classify_peer("::ffff:127.0.0.1".parse().unwrap()),
            PeerClass::Loopback
        );
        assert_eq!(
            classify_peer("::ffff:192.168.1.5".parse().unwrap()),
            PeerClass::PrivateLan
        );
        assert_eq!(
            classify_peer("::ffff:8.8.8.8".parse().unwrap()),
            PeerClass::Disallowed
        );
    }

    #[test]
    fn bind_addr_never_defaults_to_all_interfaces() {
        assert_eq!(
            ExposureMode::LoopbackOnly.bind_addr(8123),
            "127.0.0.1:8123".parse::<SocketAddr>().unwrap(),
            "默认模式必须只绑回环"
        );
        assert!(ExposureMode::LoopbackOnly
            .bind_addr(8123)
            .ip()
            .is_loopback());
        assert_eq!(
            ExposureMode::Lan.bind_addr(8123),
            "0.0.0.0:8123".parse::<SocketAddr>().unwrap()
        );
    }

    // ── 鉴权判定穷举 ───────────────────────────────────────────────────────

    /// 用已知十六进制构造令牌（子模块可访问父模块私有字段），使矩阵测试的
    /// 期望值可写死，而不依赖 `generate()` 的随机结果。
    fn tok(hex: &str) -> SessionToken {
        SessionToken(hex.to_string())
    }

    #[test]
    fn authorize_full_matrix() {
        use AuthOutcome::*;
        use ExposureMode::*;
        use PeerClass::*;
        let good = tok(&"a".repeat(TOKEN_HEX_LEN));
        let bad = "b".repeat(TOKEN_HEX_LEN);

        for mode in [LoopbackOnly, Lan] {
            for peer in [Loopback, PrivateLan, Disallowed] {
                for presented in [None, Some(bad.as_str()), Some(good.as_str())] {
                    let got = authorize(mode, peer, &good, presented);
                    let want = match (mode, peer) {
                        (_, Disallowed) => Forbidden,
                        (LoopbackOnly, PrivateLan) => Forbidden,
                        // 令牌是唯一的授权依据；对端类别只决定可达范围
                        (_, _) => {
                            if presented == Some(good.as_str()) {
                                Allowed
                            } else {
                                Unauthorized
                            }
                        }
                    };
                    assert_eq!(
                        got,
                        want,
                        "mode={mode:?} peer={peer:?} presented={:?}",
                        presented.map(|p| if p == good.as_str() {
                            "<good>"
                        } else {
                            "<bad>"
                        })
                    );
                }
            }
        }
    }

    #[test]
    fn authorize_rejects_near_miss_tokens() {
        // 只差一个字符、长度正确但内容不同、正确令牌加了空白：都必须 401
        let good = tok(&"a".repeat(TOKEN_HEX_LEN));
        let mut near = "a".repeat(TOKEN_HEX_LEN - 1);
        near.push('b');
        for wrong in [
            near,
            format!("{} ", good.as_str()),
            format!(" {}", good.as_str()),
            good.as_str().to_uppercase(),
            "a".repeat(TOKEN_HEX_LEN - 1),
            "a".repeat(TOKEN_HEX_LEN + 1),
        ] {
            for peer in [PeerClass::Loopback, PeerClass::PrivateLan] {
                assert_eq!(
                    authorize(ExposureMode::Lan, peer, &good, Some(&wrong)),
                    AuthOutcome::Unauthorized,
                    "近似令牌必须被拒: {wrong:?}（peer={peer:?}）"
                );
            }
        }
    }

    /// Batch 3.5 的核心回归断言：**回环不再免令牌**。
    ///
    /// 若该断言语义被还原（`Loopback → Allowed`），跨源 simple POST 与跨站
    /// WebSocket 会重新可用——浏览器正是以回环对端身份发起这两类请求的。
    #[test]
    fn authorize_loopback_requires_token_in_every_mode() {
        let good = tok(&"a".repeat(TOKEN_HEX_LEN));
        for mode in [ExposureMode::LoopbackOnly, ExposureMode::Lan] {
            assert_eq!(
                authorize(mode, PeerClass::Loopback, &good, None),
                AuthOutcome::Unauthorized,
                "{mode:?}: 回环对端无令牌必须 401（回环 ≠ 已认证）"
            );
            assert_eq!(
                authorize(
                    mode,
                    PeerClass::Loopback,
                    &good,
                    Some(&"c".repeat(TOKEN_HEX_LEN))
                ),
                AuthOutcome::Unauthorized,
                "{mode:?}: 回环对端错误令牌必须 401"
            );
            assert_eq!(
                authorize(mode, PeerClass::Loopback, &good, Some(good.as_str())),
                AuthOutcome::Allowed,
                "{mode:?}: 正确令牌必须放行，否则本机页面无法工作"
            );
        }
    }

    // ── 服务器 authority / Origin 策略 ─────────────────────────────────────

    fn auth_with(port: u16, addrs: &[&str]) -> ServerAuthorities {
        let list: Vec<LanCandidate> = addrs.iter().map(|a| cand(a, "nic", true)).collect();
        ServerAuthorities::new(port, &list)
    }

    #[test]
    fn host_header_accepts_only_own_authorities() {
        let a = auth_with(8123, &["10.148.63.202", "172.30.0.1"]);
        for ok in [
            "127.0.0.1:8123",
            "localhost:8123",
            "[::1]:8123",
            "10.148.63.202:8123",
            "172.30.0.1:8123",
            "LOCALHOST:8123",
        ] {
            assert!(a.host_header_allowed(ok), "{ok} 应被接受");
        }
        for bad in [
            // DNS rebinding：攻击者域名解析到 127.0.0.1，但 Host 是攻击者的域名
            "evil.example:8123",
            "evil.example",
            // 端口不符
            "127.0.0.1:8124",
            "127.0.0.1",
            "10.148.63.202:80",
            // 非本机地址
            "10.0.0.99:8123",
            "192.168.1.1:8123",
            // 畸形端口不得被截断成合法值
            "127.0.0.1:8123x",
            "127.0.0.1:",
            "127.0.0.1:8a",
            "",
            " ",
            "127.0.0.1:8123:9",
        ] {
            assert!(!a.host_header_allowed(bad), "{bad:?} 不应被接受");
        }
    }

    #[test]
    fn host_default_port_only_matches_port_80() {
        assert!(auth_with(80, &[]).host_header_allowed("127.0.0.1"));
        assert!(auth_with(80, &[]).host_header_allowed("127.0.0.1:80"));
        assert!(!auth_with(8123, &[]).host_header_allowed("127.0.0.1"));
    }

    #[test]
    fn origin_self_is_derived_from_server_state_not_host_header() {
        let a = auth_with(8123, &["10.148.63.202"]);
        for ok in [
            "http://127.0.0.1:8123",
            "http://localhost:8123",
            "http://[::1]:8123",
            "http://10.148.63.202:8123",
        ] {
            assert!(a.origin_is_self(ok), "{ok} 是服务器自身来源");
        }
        for bad in [
            "http://evil.example",
            "http://evil.example:8123",
            // 同机但不同端口 = 不同源。这正是 Batch 3.5 复现的攻击页面形态。
            "http://127.0.0.1:9999",
            "http://127.0.0.1",
            // 无 TLS，https 不构成本服务器的来源
            "https://127.0.0.1:8123",
            "http://10.0.0.99:8123",
            // Origin 为 null（sandbox / 重定向场景）
            "null",
            "",
            // 大小写与空白不得被容错
            "HTTP://127.0.0.1:8123",
            " http://127.0.0.1:8123",
        ] {
            assert!(!a.origin_is_self(bad), "{bad:?} 不得被视为本服务器来源");
        }
    }

    #[test]
    fn browser_origin_accepted_covers_self_and_tauri_only() {
        let a = auth_with(8123, &["10.148.63.202"]);
        assert!(browser_origin_accepted(&a, "http://127.0.0.1:8123"));
        assert!(browser_origin_accepted(&a, "http://10.148.63.202:8123"));
        assert!(browser_origin_accepted(&a, "http://tauri.localhost"));
        for bad in [
            "http://evil.example",
            "http://127.0.0.1:9999",
            "null",
            "http://tauri.localhost.evil.example",
        ] {
            assert!(!browser_origin_accepted(&a, bad), "{bad} 必须被拒");
        }
    }

    // ── 请求体媒体类型策略 ─────────────────────────────────────────────────

    #[test]
    fn json_content_type_policy() {
        assert!(is_json_content_type(&hdr(
            "content-type",
            "application/json"
        )));
        assert!(is_json_content_type(&hdr(
            "content-type",
            "application/json; charset=utf-8"
        )));
        assert!(is_json_content_type(&hdr(
            "content-type",
            "APPLICATION/JSON"
        )));
        assert!(is_json_content_type(&hdr(
            "content-type",
            "application/json ;charset=utf-8"
        )));
        for bad in [
            "text/plain",
            "text/plain;charset=UTF-8",
            "application/x-www-form-urlencoded",
            "multipart/form-data",
            "application/jsonp",
            "",
            " ",
        ] {
            assert!(
                !is_json_content_type(&hdr("content-type", bad)),
                "{bad:?} 不是 application/json"
            );
        }
        assert!(
            !is_json_content_type(&[]),
            "缺失 Content-Type 与声明错误同等不可接受"
        );
        assert!(
            !is_json_content_type(&hdr("x-content-type", "application/json")),
            "只看 content-type 头"
        );
    }

    #[test]
    fn json_body_required_for_every_api_write_method() {
        for m in ["POST", "PUT", "PATCH"] {
            assert!(body_must_be_json(m, "/api/route"), "{m} 必须要求 JSON");
            assert!(
                body_must_be_json(m, "/api/future"),
                "新增写入端点默认继承约束"
            );
        }
        for m in ["GET", "HEAD", "OPTIONS", "DELETE"] {
            assert!(!body_must_be_json(m, "/api/route"), "{m} 无请求体");
        }
        assert!(
            !body_must_be_json("POST", "/static/x"),
            "非 /api 路径不受约束"
        );
    }

    // ── 令牌 ───────────────────────────────────────────────────────────────

    /// 令牌形态：恰好 64 个小写十六进制字符。
    ///
    /// 该形态同时被前端 `app.js` 的 `readTokenFromFragment` 依赖（那里用等价正则
    /// 判定 fragment 是否携带值得保存的凭据），故在此以正向/反向样本锁定契约。
    fn token_shape_contract(s: &str) -> bool {
        s.len() == TOKEN_HEX_LEN
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    #[test]
    fn token_shape_and_hex() {
        let t = SessionToken::generate().unwrap();
        assert_eq!(t.as_str().len(), TOKEN_HEX_LEN);
        assert!(token_shape_contract(t.as_str()));
        assert_eq!(TOKEN_HEX_LEN, 64, "前端按 64 位十六进制校验 fragment");
        assert!(t.verify(t.as_str()));
        assert!(!t.verify(&t.as_str().to_uppercase()), "大小写不归一化");
        assert!(!t.verify(""));
        assert!(
            !t.verify(&t.as_str()[..TOKEN_HEX_LEN - 1]),
            "截断必须不通过"
        );
        // 尾随字符不得被忽略
        assert!(!t.verify(&format!("{}0", t.as_str())));
    }

    #[test]
    fn token_is_fresh_per_generation() {
        // 200 次生成不重复：若实现退化为时间戳/PID/计数器，本测试会失败
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            assert!(seen.insert(SessionToken::generate().unwrap().as_str().to_string()));
        }
    }

    #[test]
    fn token_debug_is_redacted() {
        let t = SessionToken::generate().unwrap();
        let dbg = format!("{t:?}");
        assert_eq!(dbg, "SessionToken(<redacted>)");
        assert!(!dbg.contains(t.as_str()), "Debug 输出不得包含令牌本体");
    }

    #[test]
    fn token_shape_contract_rejects_non_hex() {
        assert!(token_shape_contract(&"0123456789abcdef".repeat(4)));
        assert!(!token_shape_contract(&"g".repeat(64)), "非 hex 字符");
        assert!(!token_shape_contract(&"A".repeat(64)), "大写不接受");
        assert!(!token_shape_contract(&"a".repeat(63)));
        assert!(!token_shape_contract(&"a".repeat(65)));
        assert!(!token_shape_contract(""));
        assert!(!token_shape_contract("Bearer abc"));
    }

    #[test]
    fn ct_eq_semantics() {
        assert!(ct_eq(b"", b""));
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
        assert!(!ct_eq(b"", b"a"));
    }

    // ── 凭据提取 ───────────────────────────────────────────────────────────

    fn hdr(k: &str, v: &str) -> Vec<(String, String)> {
        vec![(k.to_string(), v.to_string())]
    }

    #[test]
    fn bearer_token_parsing() {
        assert_eq!(
            bearer_token(&hdr("authorization", "Bearer abc123")),
            Some("abc123")
        );
        assert_eq!(
            bearer_token(&hdr("authorization", "bearer abc123")),
            Some("abc123")
        );
        assert_eq!(
            bearer_token(&hdr("authorization", "BEARER abc123")),
            Some("abc123")
        );
        assert_eq!(
            bearer_token(&hdr("authorization", "Bearer   abc123  ")),
            Some("abc123")
        );
        assert_eq!(bearer_token(&hdr("authorization", "Basic abc123")), None);
        assert_eq!(bearer_token(&hdr("authorization", "Bearer ")), None);
        assert_eq!(bearer_token(&hdr("authorization", "Bearer")), None);
        assert_eq!(bearer_token(&hdr("authorization", "")), None);
        assert_eq!(
            bearer_token(&hdr("x-token", "Bearer abc")),
            None,
            "只看 authorization 头"
        );
        assert_eq!(bearer_token(&[]), None);
    }

    #[test]
    fn query_param_extraction() {
        assert_eq!(
            query_param("/ws?token=abc", "token"),
            Some("abc".to_string())
        );
        assert_eq!(
            query_param("/ws?a=1&token=abc&b=2", "token"),
            Some("abc".to_string())
        );
        assert_eq!(query_param("/ws?token=", "token"), Some(String::new()));
        assert_eq!(query_param("/ws?tokens=abc", "token"), None, "不得前缀匹配");
        assert_eq!(query_param("/ws?token=abc", "other"), None);
        assert_eq!(query_param("/ws", "token"), None);
        assert_eq!(query_param("/ws?", "token"), None);
    }

    #[test]
    fn request_path_strips_query() {
        assert_eq!(request_path("/ws?token=abc"), "/ws");
        assert_eq!(request_path("/api/snapshot"), "/api/snapshot");
        assert_eq!(request_path("/ws?"), "/ws");
    }

    // ── 候选排序与发现 ─────────────────────────────────────────────────────

    fn cand(addr: &str, name: &str, active: bool) -> LanCandidate {
        LanCandidate {
            address: addr.parse().unwrap(),
            interface: name.to_string(),
            active,
        }
    }

    #[test]
    fn rank_puts_active_first_then_numeric() {
        let v = vec![
            cand("192.168.1.5", "eth", false),
            cand("172.30.0.1", "hyperv", true),
            cand("10.0.0.7", "wifi", true),
            cand("10.0.0.3", "wifi2", true),
        ];
        let r = rank_candidates(v);
        let got: Vec<String> = r.iter().map(|c| c.address.to_string()).collect();
        assert_eq!(
            got,
            vec!["10.0.0.3", "10.0.0.7", "172.30.0.1", "192.168.1.5"]
        );
        assert!(r[0].active && r[2].active);
        assert!(!r[3].active, "非 active 排最后");
    }

    #[test]
    fn rank_is_deterministic_under_permutation() {
        let base = vec![
            cand("10.1.2.3", "a", true),
            cand("192.168.9.9", "b", false),
            cand("172.20.0.1", "c", true),
        ];
        let expect: Vec<String> = rank_candidates(base.clone())
            .iter()
            .map(|c| c.address.to_string())
            .collect();
        // 全部 6 种输入排列必须得到同一顺序
        for perm in [
            vec![0, 1, 2],
            vec![0, 2, 1],
            vec![1, 0, 2],
            vec![1, 2, 0],
            vec![2, 0, 1],
            vec![2, 1, 0],
        ] {
            let v: Vec<LanCandidate> = perm.iter().map(|&i| base[i].clone()).collect();
            let got: Vec<String> = rank_candidates(v)
                .iter()
                .map(|c| c.address.to_string())
                .collect();
            assert_eq!(got, expect, "排列 {perm:?} 改变了顺序");
        }
    }

    #[test]
    fn discovered_candidates_are_all_classified_private_lan() {
        // 关键不变量：二维码给出的地址，必须是服务端愿意接受的 PrivateLan 对端
        // 类别。若发现逻辑混入回环/链路本地/公网地址，本测试失败。
        let list = discover_lan_candidates();
        for c in &list {
            assert_eq!(
                classify_peer(IpAddr::V4(c.address)),
                PeerClass::PrivateLan,
                "候选 {} （{}）不是 PrivateLan",
                c.address,
                c.interface
            );
            assert!(!c.address.is_loopback());
        }
        // 顺序满足排序契约
        let expect: Vec<String> = rank_candidates(list.clone())
            .iter()
            .map(|c| c.address.to_string())
            .collect();
        let got: Vec<String> = list.iter().map(|c| c.address.to_string()).collect();
        assert_eq!(got, expect, "discover 返回的顺序必须已满足 rank 契约");
    }

    // ── 引导端点 ───────────────────────────────────────────────────────────

    #[test]
    fn bootstrap_returns_token_in_loopback_mode_with_lan_disabled() {
        // Batch 3.5：非 --lan 模式同样下发令牌（本机页面需要它访问动态 API）。
        // `lan_enabled` 只描述 LAN 暴露面，不描述是否已认证。
        let tok = SessionToken::generate().unwrap();
        let auth = auth_with(8123, &[]);
        let (code, body) = bootstrap_response(
            ExposureMode::LoopbackOnly,
            PeerClass::Loopback,
            None,
            &auth,
            &tok,
            &[],
        );
        assert_eq!(code, 200);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["lan_enabled"], false, "默认模式不得声称已开启 LAN");
        assert_eq!(v["token"], tok.as_str(), "本机页面必须能取到令牌");
        assert_eq!(v["port"], 8123);
        assert_eq!(v["addresses"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn bootstrap_granted_for_loopback_in_lan_mode() {
        let tok = SessionToken::generate().unwrap();
        let list = vec![
            cand("10.148.63.202", "Wi-Fi", true),
            cand("172.30.0.1", "vEthernet (Default Switch)", true),
        ];
        let auth = ServerAuthorities::new(8123, &list);
        let (code, body) = bootstrap_response(
            ExposureMode::Lan,
            PeerClass::Loopback,
            Some("http://127.0.0.1:8123"),
            &auth,
            &tok,
            &list,
        );
        assert_eq!(code, 200);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["lan_enabled"], true);
        assert_eq!(v["token"], tok.as_str());
        assert_eq!(v["port"], 8123);
        let a = v["addresses"].as_array().unwrap();
        assert_eq!(a.len(), 2, "多网卡必须全部返回，不得只给一个");
        assert_eq!(a[0]["address"], "10.148.63.202");
        assert_eq!(a[0]["interface"], "Wi-Fi");
        assert_eq!(a[0]["preferred"], true);
        assert_eq!(a[1]["preferred"], false, "只有首个候选是 preferred");
        assert!(
            a.iter().all(|x| x["address"] != "127.0.0.1"),
            "候选不得包含回环地址"
        );
    }

    #[test]
    fn bootstrap_refused_for_remote_peer_in_both_modes() {
        let tok = SessionToken::generate().unwrap();
        let list = vec![cand("10.148.63.202", "Wi-Fi", true)];
        let auth = ServerAuthorities::new(8123, &list);
        for mode in [ExposureMode::LoopbackOnly, ExposureMode::Lan] {
            for peer in [PeerClass::PrivateLan, PeerClass::Disallowed] {
                let (code, body) = bootstrap_response(mode, peer, None, &auth, &tok, &list);
                assert_eq!(code, 403, "{mode:?}/{peer:?} 必须被拒绝");
                assert!(
                    !body.contains(tok.as_str()),
                    "{mode:?}/{peer:?} 的拒绝响应不得泄露令牌"
                );
                assert!(!body.contains("10.148.63.202"), "拒绝响应不得泄露候选地址");
            }
        }
    }

    /// 关键回归：恶意页面即便以回环对端身份请求 bootstrap，也不得取得令牌。
    ///
    /// `Origin` 由浏览器写入，攻击页面无法把它伪造成 `127.0.0.1:<本端口>`——
    /// 那需要它自身就运行在该源上。
    #[test]
    fn bootstrap_refused_for_bad_browser_origin_even_from_loopback() {
        let tok = SessionToken::generate().unwrap();
        let list = vec![cand("10.148.63.202", "Wi-Fi", true)];
        let auth = ServerAuthorities::new(8123, &list);
        // 攻击页面与 nav-server 同机不同端口 → 跨源，必须被拒
        for bad in [
            "http://127.0.0.1:9999",
            "http://evil.example",
            "null",
            "http://10.0.0.99:8123",
        ] {
            let (code, body) = bootstrap_response(
                ExposureMode::Lan,
                PeerClass::Loopback,
                Some(bad),
                &auth,
                &tok,
                &list,
            );
            assert_eq!(code, 403, "Origin {bad} 不得取得令牌");
            assert!(!body.contains(tok.as_str()), "拒绝响应不得包含令牌");
        }
        // 被接受的两种浏览器来源
        for ok in ["http://127.0.0.1:8123", "http://tauri.localhost"] {
            let (code, _) = bootstrap_response(
                ExposureMode::Lan,
                PeerClass::Loopback,
                Some(ok),
                &auth,
                &tok,
                &list,
            );
            assert_eq!(code, 200, "Origin {ok} 应被接受");
        }
    }

    #[test]
    fn bootstrap_with_no_candidates_is_explicit_not_bogus() {
        // 无可用局域网地址：返回空候选，绝不退回 127.0.0.1 之类的假地址
        let tok = SessionToken::generate().unwrap();
        let auth = auth_with(8123, &[]);
        let (code, body) = bootstrap_response(
            ExposureMode::Lan,
            PeerClass::Loopback,
            None,
            &auth,
            &tok,
            &[],
        );
        assert_eq!(code, 200);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["lan_enabled"], true);
        assert!(v["addresses"].as_array().unwrap().is_empty());
    }

    // ── CORS ───────────────────────────────────────────────────────────────

    #[test]
    fn cors_no_wildcard_ever() {
        for origin in [
            None,
            Some("http://tauri.localhost"),
            Some("http://evil.example"),
            Some("null"),
            Some("http://127.0.0.1:8123"),
        ] {
            let h = cors_headers_for(origin);
            assert!(
                !h.iter().any(|(_, v)| v.contains('*')),
                "任何情况下都不得返回通配符: {origin:?} -> {h:?}"
            );
        }
    }

    #[test]
    fn cors_only_verified_tauri_origin() {
        let h = cors_headers_for(Some("http://tauri.localhost"));
        assert_eq!(
            h,
            vec![
                (
                    "Access-Control-Allow-Origin",
                    "http://tauri.localhost".to_string()
                ),
                ("Vary", "Origin".to_string()),
            ]
        );
        // 未经实测的形式不得出现在白名单里
        for wrong in [
            "tauri://localhost",
            "https://tauri.localhost",
            "http://localhost:1420",
            "http://tauri.localhost.evil.example",
            "http://tauri.localhost:80",
        ] {
            assert!(
                cors_headers_for(Some(wrong)).is_empty(),
                "{wrong} 未经实测，不得进入白名单"
            );
        }
    }

    #[test]
    fn cors_denied_origin_gets_no_headers() {
        assert!(cors_headers_for(Some("http://evil.example")).is_empty());
        assert!(cors_headers_for(Some("null")).is_empty());
        assert!(cors_headers_for(None).is_empty(), "无 Origin 不视为许可");
        // 同源请求（Origin 为服务自身）同样不需要 CORS 头——浏览器不会因此失败
        assert!(cors_headers_for(Some("http://192.168.1.9:8123")).is_empty());
    }

    #[test]
    fn preflight_only_for_allowed_origin_and_never_wildcard() {
        let ok = preflight_headers_for(Some("http://tauri.localhost"));
        let map: std::collections::HashMap<_, _> = ok.iter().cloned().collect();
        assert_eq!(map["Access-Control-Allow-Origin"], "http://tauri.localhost");
        assert_eq!(map["Access-Control-Allow-Methods"], "GET, POST, OPTIONS");
        assert_eq!(
            map["Access-Control-Allow-Headers"],
            "Authorization, Content-Type"
        );
        assert!(!map["Access-Control-Allow-Methods"].contains('*'));
        assert!(!map["Access-Control-Allow-Headers"].contains('*'));

        for bad in [None, Some("http://evil.example"), Some("null")] {
            assert!(
                preflight_headers_for(bad).is_empty(),
                "非白名单 origin 的预检不得返回许可头: {bad:?}"
            );
        }
    }

    #[test]
    fn protected_api_default_deny() {
        for p in [
            "/api/snapshot",
            "/api/route",
            "/api/metadata",
            "/api/search",
            "/api/settings",
            "/api/unknown-future-endpoint",
            "/api/",
        ] {
            assert!(is_protected_api(p), "{p} 必须受保护（默认拒绝）");
        }
        // 引导端点是唯一豁免（且自身受回环 + Origin 限制）
        assert!(!is_protected_api("/api/bootstrap"));
        assert!(!is_protected_api(BOOTSTRAP_PATH));
        // Batch 3 的旧路径已废弃：它不得作为遗留豁免继续存在
        assert!(
            is_protected_api("/api/lan-bootstrap"),
            "废弃路径必须落回默认拒绝，不得保留第二个豁免入口"
        );
        // 静态资源不鉴权（§11 威胁模型决策）
        for p in [
            "/",
            "/index.html",
            "/app.js",
            "/style.css",
            "/map.pmtiles",
            "/manifest.json",
        ] {
            assert!(!is_protected_api(p), "{p} 是静态资源");
        }
        // 前缀匹配不得误伤非 /api 路径
        assert!(!is_protected_api("/apifoo"));
        assert!(!is_protected_api("/ws"));
    }

    #[test]
    fn cors_origin_with_crlf_yields_no_headers() {
        // 注入尝试：Origin 里塞 CRLF 或任何额外内容都不可能命中白名单常量，
        // 且写出的值取自常量，响应头结构不可能被请求内容改变。
        for evil in [
            "http://tauri.localhost\r\nX-Injected: 1",
            "http://tauri.localhost\nX-Injected: 1",
            " http://tauri.localhost",
            "http://tauri.localhost ",
            "http://TAURI.localhost",
        ] {
            let h = cors_headers_for(Some(evil));
            assert!(h.is_empty(), "{evil:?} 不得命中白名单");
        }
    }
}
