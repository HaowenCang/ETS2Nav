//! P4R Batch 3：LAN 暴露面、会话令牌与 CORS 策略的判定核心。
//!
//! 结构原则：**所有安全判定都是纯函数**（`classify_peer` / `authorize` /
//! `lan_bootstrap_response` / `cors_headers_for` / `rank_candidates`），socket 与
//! HTTP 处理层只负责调用它们并把结果落成状态码。这样安全边界可以被穷举式单元
//! 测试覆盖，而不必依赖启动真实服务器；真实 socket 行为另由集成套件覆盖。
//!
//! 威胁模型（本轮范围，见 validation report）：同一局域网内**未授权的第三方**
//! 读取实时导航状态（位置/目的地/路线）或改写导航目的地。不覆盖：公网暴露、
//! 本机其他用户进程、TLS/中间人、供应链。因此不做「公网服务器」级设计。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// 会话令牌的随机字节数（256-bit 安全强度）。
pub const TOKEN_BYTES: usize = 32;
/// 令牌的十六进制字符数。选用 hex 而非 base64：URL query / fragment 无需 percent
/// 编码即可安全携带，测试断言也无需处理转义。
pub const TOKEN_HEX_LEN: usize = TOKEN_BYTES * 2;

// ─── 来源分类 ────────────────────────────────────────────────────────────────

/// 连接来源类别。分类只依赖对端 IP，不依赖任何请求内容——请求内容由未授权方
/// 完全控制，不能作为安全判定的输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerClass {
    /// 127.0.0.0/8。本机 Browser/Tauri dev 路径，两种模式下都免令牌。
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExposureMode {
    /// 只绑定 127.0.0.1。局域网不可达，因此不生成令牌。
    LoopbackOnly,
    /// 绑定 0.0.0.0。私网对端必须携带令牌；回环对端豁免，避免本机
    /// Browser/Tauri 自举死循环。
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
/// 规则表：
/// | 模式         | Loopback | PrivateLan        | Disallowed |
/// |--------------|----------|-------------------|------------|
/// | LoopbackOnly | Allowed  | Forbidden         | Forbidden  |
/// | Lan          | Allowed  | 令牌正确→Allowed  | Forbidden  |
///
/// 两处刻意的设计：
/// - `LoopbackOnly` 下私网对端判 `Forbidden` 而非 `Unauthorized`：默认模式下
///   私网对端根本不该连进来（bind 层已挡住），判 `Forbidden` 是纵深防御，
///   且语义正确——它不是「缺令牌」，而是「这个来源不该出现」。
/// - 令牌**只在** `Lan + PrivateLan` 时参与判定：回环豁免是刻意的，否则本机
///   页面无法自举（它拿不到令牌就无法访问 `/api/lan-bootstrap`）。
pub fn authorize(
    mode: ExposureMode,
    peer: PeerClass,
    expected: Option<&SessionToken>,
    presented: Option<&str>,
) -> AuthOutcome {
    if peer == PeerClass::Disallowed {
        return AuthOutcome::Forbidden;
    }
    if peer == PeerClass::Loopback {
        return AuthOutcome::Allowed;
    }
    // 以下均为 PrivateLan
    if mode == ExposureMode::LoopbackOnly {
        return AuthOutcome::Forbidden;
    }
    match (expected, presented) {
        (Some(exp), Some(got)) if exp.verify(got) => AuthOutcome::Allowed,
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

/// `/api/lan-bootstrap` 响应：(HTTP 状态码, JSON 体)。
///
/// 这是整个模型的**唯一**令牌出口，且只对回环对端开放。刻意不把令牌放进
/// `/api/metadata`、`index.html` 或 JS bundle——那些资源对任何 LAN 对端可读
/// （静态资源不鉴权，见 §11 威胁模型决策），放进去等于取消鉴权。
///
/// - 非 `--lan`：`200 {"enabled": false}`，且**不生成也不返回**令牌。
/// - `--lan` + 回环：`200 {"enabled": true, port, addresses[], token}`。
/// - `--lan` + 私网/其他：`403`，响应体不含任何令牌信息。
///
/// `addresses` 为对象数组（含 `interface` / `active` / `preferred`），比
/// 纯字符串数组多出的字段是 UI 在多网卡下做知情选择所必需的：任务书示例形如
/// `["192.168.1.123"]`，但仅凭地址字符串无法区分 Wi-Fi 与 Hyper-V 虚拟网卡，
/// UI 只能盲选。表示只有一种，不额外附带冗余的字符串数组。
pub fn lan_bootstrap_response(
    mode: ExposureMode,
    peer: PeerClass,
    port: u16,
    token: Option<&SessionToken>,
    candidates: &[LanCandidate],
) -> (u16, String) {
    if mode == ExposureMode::LoopbackOnly {
        return (200, r#"{"enabled":false}"#.to_string());
    }
    if peer != PeerClass::Loopback {
        return (
            403,
            r#"{"error":"lan bootstrap is loopback-only"}"#.to_string(),
        );
    }
    let Some(token) = token else {
        // `--lan` 下令牌必然存在；缺失属内部状态错误，按拒绝处理而非放行。
        return (
            403,
            r#"{"error":"lan enabled but no session token"}"#.to_string(),
        );
    };
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
        "enabled": true,
        "port": port,
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
/// 采用**默认拒绝**：`/api/` 下的一切路径都受保护，唯一豁免是回环限定的
/// `/api/lan-bootstrap`。任务书要求「至少保护」五个既有端点；按枚举放行会让
/// 将来新增的 `/api/*` 端点默认处于未鉴权状态，而按前缀拒绝使新增端点默认安全。
///
/// 静态资源（`/`、`index.html`、JS/CSS/vendor/字体/`map.pmtiles`/manifest）不在
/// 保护范围内，这是刻意的威胁模型决策：手机必须先取到客户端代码，才能读取 URL
/// fragment 中的令牌并携带它；且地图档案不是实时个人状态。本轮的防护目标是
/// **动态导航状态与控制 API**，不是「LAN 服务全部资源均需认证」。
pub fn is_protected_api(path: &str) -> bool {
    path.starts_with("/api/") && path != "/api/lan-bootstrap"
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
                let exp = if mode == Lan { Some(&good) } else { None };
                for presented in [None, Some(bad.as_str()), Some(good.as_str())] {
                    let got = authorize(mode, peer, exp, presented);
                    let want = match (mode, peer) {
                        (_, Disallowed) => Forbidden,
                        (_, Loopback) => Allowed,
                        (LoopbackOnly, PrivateLan) => Forbidden,
                        (Lan, PrivateLan) => {
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
            assert_eq!(
                authorize(
                    ExposureMode::Lan,
                    PeerClass::PrivateLan,
                    Some(&good),
                    Some(&wrong)
                ),
                AuthOutcome::Unauthorized,
                "近似令牌必须被拒: {wrong:?}"
            );
        }
    }

    #[test]
    fn authorize_loopback_exempt_in_lan_mode() {
        // 关键：LAN 模式下本机页面必须免令牌，否则 /api/lan-bootstrap 自举死循环
        let good = tok(&"a".repeat(TOKEN_HEX_LEN));
        assert_eq!(
            authorize(ExposureMode::Lan, PeerClass::Loopback, Some(&good), None),
            AuthOutcome::Allowed
        );
    }

    #[test]
    fn authorize_lan_without_expected_token_never_allows_private_peer() {
        // 内部状态异常（LAN 模式却没生成令牌）不得退化为放行
        assert_eq!(
            authorize(
                ExposureMode::Lan,
                PeerClass::PrivateLan,
                None,
                Some("anything")
            ),
            AuthOutcome::Unauthorized
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
    fn bootstrap_disabled_without_lan() {
        let tok = SessionToken::generate().unwrap();
        let (code, body) = lan_bootstrap_response(
            ExposureMode::LoopbackOnly,
            PeerClass::Loopback,
            8123,
            Some(&tok),
            &[],
        );
        assert_eq!(code, 200);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["enabled"], false);
        assert!(
            !body.contains(tok.as_str()),
            "非 LAN 模式不得以任何形式返回令牌"
        );
        assert!(v.get("token").is_none());
    }

    #[test]
    fn bootstrap_granted_for_loopback_in_lan_mode() {
        let tok = SessionToken::generate().unwrap();
        let list = vec![
            cand("10.148.63.202", "Wi-Fi", true),
            cand("172.30.0.1", "vEthernet (Default Switch)", true),
        ];
        let (code, body) = lan_bootstrap_response(
            ExposureMode::Lan,
            PeerClass::Loopback,
            8123,
            Some(&tok),
            &list,
        );
        assert_eq!(code, 200);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["enabled"], true);
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
    fn bootstrap_refused_for_remote_peer() {
        let tok = SessionToken::generate().unwrap();
        let list = vec![cand("10.148.63.202", "Wi-Fi", true)];
        for peer in [PeerClass::PrivateLan, PeerClass::Disallowed] {
            let (code, body) =
                lan_bootstrap_response(ExposureMode::Lan, peer, 8123, Some(&tok), &list);
            assert_eq!(code, 403, "{peer:?} 必须被拒绝");
            assert!(
                !body.contains(tok.as_str()),
                "{peer:?} 的拒绝响应不得泄露令牌"
            );
            assert!(!body.contains("10.148.63.202") || peer == PeerClass::PrivateLan);
        }
    }

    #[test]
    fn bootstrap_with_no_candidates_is_explicit_not_bogus() {
        // 无可用局域网地址：返回空候选，绝不退回 127.0.0.1 之类的假地址
        let tok = SessionToken::generate().unwrap();
        let (code, body) = lan_bootstrap_response(
            ExposureMode::Lan,
            PeerClass::Loopback,
            8123,
            Some(&tok),
            &[],
        );
        assert_eq!(code, 200);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["enabled"], true);
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
        // 引导端点单独处理（回环限定），不作为普通受保护 API
        assert!(!is_protected_api("/api/lan-bootstrap"));
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
