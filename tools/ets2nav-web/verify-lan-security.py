#!/usr/bin/env python3
"""nav-server 暴露面 / 鉴权 / Origin / Host / CORS 集成验证（P4R Batch 3 §20 S1–S9，
Batch 3.5 扩展为 S1–S11）。

这不是单元测试：全部断言都打在**真实运行的 nav-core-cli server 进程**上，经真实
TCP/HTTP/WebSocket 连接，并且刻意区分两种来源：

    127.0.0.1        -> Loopback     （Batch 3.5 起**同样要求令牌**）
    <本机 RFC1918>   -> PrivateLan   （要求令牌）
    <本机非私网地址> -> Disallowed   （连接层即被拒绝）

关键在于：从本机连到自己的私网地址时，内核选用的源地址就是该私网地址，因此服务端
看到的对端**确实**是 PrivateLan，而不是回环。这使单机也能真实执行「远端来源」矩阵，
无需第二台设备，也不是靠桩件伪造来源。

Batch 3.5 的核心修正是：**「TCP 对端是 127.0.0.1」不等于「请求意图可信」**。浏览器
可以被远程页面驱使去连回环，服务端看到的对端同样是 Loopback。因此本脚本新增的
S3b（回环 API 令牌矩阵）、S10（Host 策略）与 S11（Origin 策略）与既有断言同等重要，
且 S1/S4/S5 中依赖「回环免令牌」的旧断言已按新模型改写。

用法:
    verify-lan-security.py <config.json>
    verify-lan-security.py --selftest

config.json 由 scripts/run-security-test.mjs 生成：
    {"defaultPort":N, "lanPort":N, "tokenLive":"...", "tokenStale":"...",
     "candidates":["10.x.x.x", ...], "disallowedCandidates":[...]}

退出码：0 全部通过；1 存在 FAIL；2 自检失败；3 存在 NOT VERIFIED
（不计为通过——安全结论不能建立在未执行的检查上）。
"""
import base64
import json
import os
import socket
import struct
import sys
import time

FAIL = []
NOT_VERIFIED = []
SKIP = []

# ─── 自检：判定机制本身必须先可信 ────────────────────────────────────────────


def parse_response(raw):
    """bytes → (status:int, headers:dict(小写键), body:bytes)。"""
    head, _, body = raw.partition(b"\r\n\r\n")
    lines = head.decode("latin-1").split("\r\n")
    if not lines or not lines[0].startswith("HTTP/"):
        raise ValueError(f"非 HTTP 响应: {raw[:60]!r}")
    parts = lines[0].split()
    status = int(parts[1])
    headers = {}
    for ln in lines[1:]:
        if ":" in ln:
            k, v = ln.split(":", 1)
            headers[k.strip().lower()] = v.strip()
    return status, headers, body


def has_cors_wildcard(headers):
    """任何 Access-Control-* 头出现 `*` 都要能被检出（M3 mutation 的判定依据）。"""
    for k, v in headers.items():
        if k.startswith("access-control-") and "*" in v:
            return True
    return False


def is_token_shaped(s):
    """令牌契约：64 位小写十六进制。S1/S2 用它判断 bootstrap 是否真的下发了令牌。"""
    return isinstance(s, str) and len(s) == 64 and all(c in "0123456789abcdef" for c in s)


def selftest():
    """验证本脚本的解析与判定逻辑本身正确——否则「PASS」可能只是解析器坏了。"""
    cases = []

    def eq(name, got, want):
        cases.append((name, got == want, f"got={got!r} want={want!r}"))

    st, h, b = parse_response(
        b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n"
        b"Access-Control-Allow-Origin: *\r\nContent-Length: 2\r\n\r\n{}"
    )
    eq("parse status", st, 401)
    eq("parse body", b, b"{}")
    eq("parse header lowercased", h.get("content-type"), "application/json")
    eq("wildcard detected", has_cors_wildcard(h), True)

    st, h, b = parse_response(
        b"HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: http://tauri.localhost\r\n\r\n"
    )
    eq("204 empty body", b, b"")
    eq("specific origin not flagged as wildcard", has_cors_wildcard(h), False)

    st, h, b = parse_response(b"HTTP/1.1 200 OK\r\n\r\nx")
    eq("no headers", h, {})

    # 具名 origin 里含 `*` 的畸形值必须仍被检出（防止用 `*` 伪装成 origin）
    _, h, _ = parse_response(b"HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\n\r\n")
    eq("bare star flagged", has_cors_wildcard(h), True)

    # 令牌形态判定：正向与反向样本都必须正确，否则 S1/S2 可能把畸形值当合法令牌
    eq("token shaped 64 hex", is_token_shaped("a" * 64), True)
    eq("token shaped rejects uppercase", is_token_shaped("A" * 64), False)
    eq("token shaped rejects short", is_token_shaped("a" * 63), False)
    eq("token shaped rejects non-str", is_token_shaped(None), False)
    eq("token shaped rejects non-hex", is_token_shaped("g" * 64), False)

    bad = 0
    for name, ok, detail in cases:
        print(f"[{'PASS' if ok else 'FAIL'}] selftest: {name}" + ("" if ok else f" — {detail}"))
        bad += 0 if ok else 1
    print(f"selftest: {len(cases) - bad}/{len(cases)} passed")
    return 2 if bad else 0


if __name__ == "__main__" and "--selftest" in sys.argv:
    sys.exit(selftest())

if len(sys.argv) < 2:
    print(__doc__)
    sys.exit(2)

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    CFG = json.load(fh)

DEFAULT_PORT = int(CFG["defaultPort"])
LAN_PORT = int(CFG["lanPort"])
# 默认（非 --lan）模式进程的令牌。Batch 3.5：该进程同样生成并下发令牌。
DEFAULT_TOKEN = CFG.get("defaultToken")
TOKEN_LIVE = CFG["tokenLive"]
TOKEN_STALE = CFG["tokenStale"]
CANDIDATES = CFG.get("candidates") or []
LAN_IP = CANDIDATES[0] if CANDIDATES else None
DATASET_DIR_HINT = CFG.get("datasetDirHint", "europe-v5")
# 非 RFC1918 的本机地址（VPN/隧道/链路本地），用于真实执行 Disallowed 来源断言
DISALLOWED_CANDIDATES = CFG.get("disallowedCandidates") or []


# ─── 原始 HTTP / WS 客户端 ───────────────────────────────────────────────────


def http(host, port, method, path, headers=None, body=b"", timeout=8.0):
    """发一个 HTTP 请求。返回 (status, headers, body_text)；连接失败返回 None。

    `Host` 默认写成 `{host}:{port}`，但可经 headers 覆盖——S10 需要发送与目标
    authority 不符的 Host，以验证服务端不把它当作信任来源。
    """
    try:
        s = socket.create_connection((host, port), timeout=timeout)
    except OSError:
        return None
    try:
        hdrs = dict(headers or {})
        hdrs.setdefault("Host", f"{host}:{port}")
        hdrs.setdefault("Connection", "close")
        if body:
            hdrs.setdefault("Content-Length", str(len(body)))
        req = f"{method} {path} HTTP/1.1\r\n"
        req += "".join(f"{k}: {v}\r\n" for k, v in hdrs.items())
        req += "\r\n"
        s.sendall(req.encode() + body)
        raw = b""
        clen = None
        while True:
            if b"\r\n\r\n" in raw:
                if clen is None:
                    head = raw.split(b"\r\n\r\n", 1)[0]
                    for ln in head.decode("latin-1").split("\r\n")[1:]:
                        if ln.lower().startswith("content-length:"):
                            clen = int(ln.split(":", 1)[1].strip())
                if clen is not None and len(raw.split(b"\r\n\r\n", 1)[1]) >= clen:
                    break
                if clen is None:
                    break
            chunk = s.recv(65536)
            if not chunk:
                break
            raw += chunk
    except (socket.timeout, OSError):
        return None
    finally:
        s.close()
    try:
        st, h, b = parse_response(raw)
    except ValueError:
        return None
    return st, h, b.decode("utf-8", "replace")


#: 表示「不发送 Origin 头」的哨兵值（原生客户端路径）。
NO_ORIGIN = object()


def ws_attempt(host, port, path, timeout=8.0, origin=NO_ORIGIN, host_header=None):
    """尝试 WS 握手。返回 (status, headers, sock)；握手完成则 sock 可直接读帧。

    `origin` 默认取同源（`http://{host}:{port}`），可显式传入任意值以验证 Origin
    策略，或传 `NO_ORIGIN` 表示完全不发送 Origin（原生客户端）。

    `host_header` 可覆盖 Host 头（S10 Host 策略）。
    """
    try:
        s = socket.create_connection((host, port), timeout=timeout)
    except OSError:
        return (None, {}, None)
    key = base64.b64encode(os.urandom(16)).decode()
    hdr = f"GET {path} HTTP/1.1\r\nHost: {host_header or f'{host}:{port}'}\r\n"
    if origin is not NO_ORIGIN:
        hdr += f"Origin: {origin}\r\n"
    hdr += (
        f"Upgrade: websocket\r\nConnection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    s.sendall(hdr.encode())
    s.settimeout(timeout)
    raw = b""
    try:
        while b"\r\n\r\n" not in raw:
            chunk = s.recv(4096)
            if not chunk:
                break
            raw += chunk
    except (socket.timeout, OSError):
        pass
    if not raw:
        s.close()
        return (None, {}, None)
    status, headers, _ = parse_response(raw.split(b"\r\n\r\n", 1)[0] + b"\r\n\r\n")
    if status != 101:
        s.close()
        return (status, headers, None)
    return (status, headers, s)


def ws_recv_frame(sock, timeout=10.0):
    sock.settimeout(timeout)

    def rd(n):
        buf = b""
        while len(buf) < n:
            c = sock.recv(n - len(buf))
            if not c:
                raise OSError("socket closed")
            buf += c
        return buf

    hdr = rd(2)
    ln = hdr[1] & 0x7F
    if ln == 126:
        ln = struct.unpack(">H", rd(2))[0]
    elif ln == 127:
        ln = struct.unpack(">Q", rd(8))[0]
    return hdr[0] & 0x0F, rd(ln) if ln else b""


def ws_collect(sock, want=1, seconds=12.0, predicate=None):
    """收集 WS 文本帧，直到满足 predicate 或超时。返回解析出的 JSON 列表。"""
    out = []
    end = time.time() + seconds
    while time.time() < end:
        try:
            op, payload = ws_recv_frame(sock, timeout=max(0.5, end - time.time()))
        except (socket.timeout, OSError):
            break
        if op == 0x8:
            break
        if op == 0x1:
            try:
                d = json.loads(payload.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError):
                continue
            out.append(d)
            if predicate and predicate(d):
                return out
            if not predicate and len(out) >= want:
                return out
    return out


# ─── 判定与输出 ──────────────────────────────────────────────────────────────


def check(name, ok, detail=""):
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""))
    if not ok:
        FAIL.append(name)


def not_verified(name, why):
    print(f"[NOT VERIFIED] {name} — {why}")
    NOT_VERIFIED.append(name)


def skip(name, why):
    print(f"[SKIP] {name} — {why}")
    SKIP.append(name)


def bearer(tok):
    return {"Authorization": f"Bearer {tok}"}


def wait_for_up(host, port, seconds=60.0):
    """等待服务器可服务：以 bootstrap 返回 200 为就绪判据。"""
    end = time.time() + seconds
    while time.time() < end:
        r = http(host, port, "GET", "/api/bootstrap", timeout=3.0)
        if r is not None and r[0] == 200:
            return True
        time.sleep(0.3)
    return False


ROUTE_A = {"from": [-58456, 32832], "to": [-52925, 36510]}
ROUTE_B = {"from": [-58456, 32832], "to": [-57000, 34500]}

JSON_CT = {"Content-Type": "application/json"}


def post_route(host, port, headers, payload, content_type="application/json"):
    h = dict(headers or {})
    h["Content-Type"] = content_type
    return http(host, port, "POST", "/api/route", h, json.dumps(payload).encode())


# ─── 主流程 ──────────────────────────────────────────────────────────────────


def main():
    print("=" * 72)
    print("nav-server 暴露面 / 鉴权 / Origin / Host / CORS 集成验证（S1–S11）")
    print("=" * 72)
    print(f"default 模式端口 : {DEFAULT_PORT}")
    print(f"--lan   模式端口 : {LAN_PORT}")
    print(f"候选私网地址     : {CANDIDATES or '(无)'}")
    print(f"对端类别测试地址 : {LAN_IP or '(无——远端矩阵不可执行)'}")
    print()

    # ── S6 令牌轮换 ────────────────────────────────────────────────────────
    if TOKEN_STALE == TOKEN_LIVE:
        check("S6 令牌每次进程启动重新生成", False, "两次启动的令牌相同")
    else:
        check("S6 令牌每次进程启动重新生成", True,
              f"len={len(TOKEN_LIVE)} 且与上一进程不同")
    check("S6 令牌为 256-bit（64 位小写十六进制）", is_token_shaped(TOKEN_LIVE),
          f"len={len(TOKEN_LIVE)}")

    # ── S1 默认 bind 与默认模式令牌 ────────────────────────────────────────
    # Batch 3.5：默认（非 --lan）模式**同样生成并下发令牌**，且动态 API 同样要求
    # 令牌。旧断言「默认模式不生成令牌 / 不含 token 字段」已按新模型删除——那正是
    # 「回环免认证」模型在默认模式下的形态。
    check("S1 默认模式同样下发会话令牌（两种模式一致）",
          is_token_shaped(DEFAULT_TOKEN),
          f"len={len(DEFAULT_TOKEN) if DEFAULT_TOKEN else 0}")
    if DEFAULT_TOKEN is None:
        not_verified("S1 默认模式回环 API 令牌矩阵",
                     "运行器未提供 defaultToken，无法构造带令牌请求")
    else:
        r = http("127.0.0.1", DEFAULT_PORT, "GET", "/api/snapshot")
        check("S1 默认模式回环无令牌 401（回环 ≠ 已认证）",
              r is not None and r[0] == 401, f"status={r[0] if r else 'CONN-FAIL'}")
        r = http("127.0.0.1", DEFAULT_PORT, "GET", "/api/snapshot", bearer(DEFAULT_TOKEN))
        check("S1 默认模式回环带令牌 200",
              r is not None and r[0] == 200, f"status={r[0] if r else 'CONN-FAIL'}")
    if LAN_IP is None:
        not_verified("S1 默认模式私网地址不可达", "本机无 RFC1918 候选地址，无法构造私网来源连接")
    else:
        r2 = http(LAN_IP, DEFAULT_PORT, "GET", "/api/snapshot")
        check("S1 默认模式私网地址不可达（只绑 127.0.0.1）", r2 is None,
              f"status={r2[0] if r2 else 'CONN-FAIL(期望)'}")
    # 默认模式的 bootstrap 只描述暴露面，不下发可选地址
    r = http("127.0.0.1", DEFAULT_PORT, "GET", "/api/bootstrap")
    body = r[2] if r else ""
    try:
        bv = json.loads(body)
    except json.JSONDecodeError:
        bv = {}
    check("S1 默认模式 bootstrap 为 lan_enabled:false",
          r is not None and r[0] == 200 and bv.get("lan_enabled") is False, body[:120])
    check("S1 默认模式 bootstrap 不下发局域网候选地址",
          bv.get("addresses") == [], f"addresses={bv.get('addresses')!r}")
    check("S1 bootstrap 响应带 no-store 与 nosniff",
          r is not None and r[1].get("cache-control") == "no-store"
          and r[1].get("x-content-type-options") == "nosniff"
          and r[1].get("referrer-policy") == "no-referrer",
          f"hdr={ {k: r[1].get(k) for k in ('cache-control', 'x-content-type-options', 'referrer-policy')} if r else None}")
    # 退役路径必须落回默认拒绝，不得保留第二个豁免入口
    r = http("127.0.0.1", DEFAULT_PORT, "GET", "/api/lan-bootstrap")
    check("S1 退役路径 /api/lan-bootstrap 不再是豁免端点",
          r is not None and r[0] == 401, f"status={r[0] if r else 'CONN-FAIL'}")

    # ── S2 bootstrap（唯一令牌出口）────────────────────────────────────────
    r = http("127.0.0.1", LAN_PORT, "GET", "/api/bootstrap")
    boot = {}
    if r and r[0] == 200:
        try:
            boot = json.loads(r[2])
        except json.JSONDecodeError:
            boot = {}
    check("S2 回环 bootstrap 返回 lan_enabled:true", boot.get("lan_enabled") is True,
          str(boot)[:160])
    check("S2 回环 bootstrap 返回可用令牌",
          boot.get("token") == TOKEN_LIVE and is_token_shaped(TOKEN_LIVE))
    addrs = boot.get("addresses") or []
    check("S2 bootstrap 返回至少一个候选地址", len(addrs) >= 1,
          json.dumps(addrs, ensure_ascii=False))
    check("S2 候选地址均为 RFC1918 且非回环",
          bool(addrs) and all(a["address"] != "127.0.0.1" for a in addrs)
          and all(a["address"].startswith(("10.", "192.168."))
                  or (a["address"].startswith("172.") and 16 <= int(a["address"].split(".")[1]) <= 31)
                  for a in addrs))
    check("S2 首个候选被标为 preferred",
          bool(addrs) and addrs[0].get("preferred") is True
          and all(a.get("preferred") is False for a in addrs[1:]))
    # bootstrap 是**唯一**豁免端点：POST 也不得被接受
    r = http("127.0.0.1", LAN_PORT, "POST", "/api/bootstrap", JSON_CT, b"{}")
    check("S2 bootstrap 只接受 GET（405）",
          r is not None and r[0] == 405, f"status={r[0] if r else 'CONN-FAIL'}")

    if LAN_IP is None:
        not_verified("S2 远端 bootstrap 被拒（403）", "无私网地址")
        not_verified("S2 远端拒绝响应不泄露令牌", "无私网地址")
    else:
        r = http(LAN_IP, LAN_PORT, "GET", "/api/bootstrap")
        check("S2 远端 bootstrap 被拒（403）", r is not None and r[0] == 403,
              f"status={r[0] if r else 'CONN-FAIL'}")
        check("S2 远端拒绝响应不泄露令牌",
              r is not None and TOKEN_LIVE not in r[2] and "token" not in r[2].lower())

    # ── S3 远端 API 鉴权 ───────────────────────────────────────────────────
    if LAN_IP is None:
        for n in ["S3 snapshot 无令牌 401", "S3 snapshot 错令牌 401",
                  "S3 snapshot 正确令牌 200", "S3 route 无令牌 401",
                  "S3 route 错令牌 401"]:
            not_verified(n, "无私网地址，无法构造远端来源")
    else:
        r = http(LAN_IP, LAN_PORT, "GET", "/api/snapshot")
        check("S3 snapshot 无令牌 401", r is not None and r[0] == 401,
              f"status={r[0] if r else 'CONN-FAIL'}")
        r = http(LAN_IP, LAN_PORT, "GET", "/api/snapshot", bearer("b" * 64))
        check("S3 snapshot 错令牌 401", r is not None and r[0] == 401,
              f"status={r[0] if r else 'CONN-FAIL'}")
        r = http(LAN_IP, LAN_PORT, "GET", "/api/snapshot", bearer(TOKEN_LIVE))
        check("S3 snapshot 正确令牌 200", r is not None and r[0] == 200,
              f"status={r[0] if r else 'CONN-FAIL'}")
        # query 令牌对 HTTP API 无效（只有 /ws 接受 query）
        r = http(LAN_IP, LAN_PORT, "GET", f"/api/snapshot?token={TOKEN_LIVE}")
        check("S3 HTTP API 不接受 query 令牌", r is not None and r[0] == 401,
              f"status={r[0] if r else 'CONN-FAIL'}")
        # 伪造 Origin 不构成授权（Origin 不是 authentication）
        r = http(LAN_IP, LAN_PORT, "GET", "/api/snapshot",
                 {"Origin": "http://tauri.localhost"})
        check("S3 伪造白名单 Origin 仍 401（Origin 非授权）",
              r is not None and r[0] == 401, f"status={r[0] if r else 'CONN-FAIL'}")

        r = post_route(LAN_IP, LAN_PORT, None, ROUTE_A)
        check("S3 route 无令牌 401", r is not None and r[0] == 401,
              f"status={r[0] if r else 'CONN-FAIL'}")
        r = post_route(LAN_IP, LAN_PORT, bearer("b" * 64), ROUTE_A)
        check("S3 route 错令牌 401", r is not None and r[0] == 401,
              f"status={r[0] if r else 'CONN-FAIL'}")

    # ── S3b 回环 API 令牌矩阵（Batch 3.5 新增，本批核心）──────────────────
    # 五个受保护端点：回环对端**无令牌一律 401，带令牌一律 200**。
    # 这条断言若被还原为「回环免令牌」，跨源 simple POST（BLS-01）即重新可用。
    if DEFAULT_TOKEN is None:
        not_verified("S3b 回环受保护端点令牌矩阵", "运行器未提供 defaultToken")
    else:
        for path in ("/api/snapshot", "/api/metadata", "/api/settings", "/api/search"):
            r = http("127.0.0.1", DEFAULT_PORT, "GET", path)
            check(f"S3b 回环无令牌 401: {path}", r is not None and r[0] == 401,
                  f"status={r[0] if r else 'CONN-FAIL'}")
            r = http("127.0.0.1", DEFAULT_PORT, "GET", path, bearer(DEFAULT_TOKEN))
            check(f"S3b 回环带令牌 200: {path}", r is not None and r[0] == 200,
                  f"status={r[0] if r else 'CONN-FAIL'}")
        r = post_route("127.0.0.1", DEFAULT_PORT, None, ROUTE_A)
        check("S3b 回环 POST /api/route 无令牌 401",
              r is not None and r[0] == 401, f"status={r[0] if r else 'CONN-FAIL'}")
        r = post_route("127.0.0.1", DEFAULT_PORT, bearer(DEFAULT_TOKEN), ROUTE_A)
        check("S3b 回环 POST /api/route 带令牌 200",
              r is not None and r[0] == 200, f"status={r[0] if r else 'CONN-FAIL'}")

    # ── S3c /api/route 媒体类型策略（CSRF 纵深防御）────────────────────────
    # 浏览器只对 CORS safelisted content type 允许无预检的跨源发送；要求
    # application/json 使跨源写入在预检层即不可达，而不只依赖令牌。
    if DEFAULT_TOKEN is None:
        not_verified("S3c route 媒体类型策略", "运行器未提供 defaultToken")
    else:
        for bad in ("text/plain", "text/plain;charset=UTF-8",
                    "application/x-www-form-urlencoded"):
            r = post_route("127.0.0.1", DEFAULT_PORT, bearer(DEFAULT_TOKEN),
                           ROUTE_A, content_type=bad)
            check(f"S3c 错误 Content-Type 被拒 415: {bad}",
                  r is not None and r[0] == 415, f"status={r[0] if r else 'CONN-FAIL'}")
        r = post_route("127.0.0.1", DEFAULT_PORT, bearer(DEFAULT_TOKEN), ROUTE_A,
                       content_type="application/json; charset=utf-8")
        check("S3c application/json; charset=utf-8 被接受",
              r is not None and r[0] == 200, f"status={r[0] if r else 'CONN-FAIL'}")

    # ── S4 未授权 route 不得产生副作用 ─────────────────────────────────────
    # 观测通道：回环 WS 客户端（Batch 3.5 起同样需要令牌）接收 map_state 广播。
    observer_status, _, obs = ws_attempt("127.0.0.1", LAN_PORT,
                                         "/ws?token=" + TOKEN_LIVE)
    check("S4 观测通道建立（回环 WS + 令牌 101）", observer_status == 101,
          f"status={observer_status}")
    if obs is None:
        not_verified("S4 未授权 route 无副作用", "观测通道不可用，无法证明副作用缺失")
    else:
        warm = ws_collect(obs, seconds=20.0,
                          predicate=lambda d: d.get("type") == "vehicle")
        channel_ok = any(d.get("type") == "vehicle" for d in warm)
        check("S4 观测通道确实在收帧（否则负向断言无意义）", channel_ok,
              f"frames={len(warm)}")
        if LAN_IP is None:
            not_verified("S4 未授权 route 无副作用", "无私网地址")
        elif not channel_ok:
            not_verified("S4 未授权 route 无副作用", "观测通道未收到 vehicle 帧")
        else:
            # 1) 未授权请求（无令牌）
            ru = post_route(LAN_IP, LAN_PORT, None, ROUTE_A)
            check("S4 未授权 POST /api/route 被拒", ru is not None and ru[0] == 401,
                  f"status={ru[0] if ru else 'CONN-FAIL'}")
            seen = ws_collect(obs, seconds=6.0,
                              predicate=lambda d: d.get("type") == "map_state")
            maps = [d for d in seen if d.get("type") == "map_state"]
            check("S4 未授权请求未触达 pending_dest（无 map_state 广播）",
                  len(maps) == 0, f"map_state={maps}")
            # 2) 正向对照：同一请求带正确令牌必须真的产生 map_state——
            #    没有这一步，「无广播」可能只是观测通道或路线本身不工作。
            ra = post_route(LAN_IP, LAN_PORT, bearer(TOKEN_LIVE), ROUTE_B)
            check("S4 授权 POST /api/route 正常处理",
                  ra is not None and ra[0] == 200, f"status={ra[0] if ra else 'CONN-FAIL'}")
            seen2 = ws_collect(obs, seconds=25.0,
                               predicate=lambda d: d.get("type") == "map_state")
            maps2 = [d for d in seen2 if d.get("type") == "map_state"]
            check("S4 正向对照：授权请求确实改变目的地（map_state 广播）",
                  len(maps2) >= 1, f"map_state={maps2}")
            if maps2:
                got = maps2[-1].get("destination")
                near = (isinstance(got, list) and len(got) == 2
                        and abs(got[0] - ROUTE_B["to"][0]) < 1.0
                        and abs(got[1] - ROUTE_B["to"][1]) < 1.0)
                check("S4 map_state 目的地为授权请求的目标（非未授权目标）",
                      near, f"destination={got} expected≈{ROUTE_B['to']}")
        try:
            obs.close()
        except OSError:
            pass

    # ── S5 WebSocket 鉴权（两种来源、两种模式）─────────────────────────────
    if LAN_IP is None:
        for n in ["S5 远端 /ws 无令牌不升级", "S5 远端 /ws 错令牌不升级",
                  "S5 远端 /ws 正确令牌 101 + 帧流"]:
            not_verified(n, "无私网地址")
    else:
        st, _, _ = ws_attempt(LAN_IP, LAN_PORT, "/ws")
        check("S5 远端 /ws 无令牌不升级（401，且无 101）", st == 401, f"status={st}")
        st, _, _ = ws_attempt(LAN_IP, LAN_PORT, "/ws?token=" + "b" * 64)
        check("S5 远端 /ws 错令牌不升级（401，且无 101）", st == 401, f"status={st}")
        st, hdr, sock = ws_attempt(LAN_IP, LAN_PORT, "/ws?token=" + TOKEN_LIVE)
        check("S5 远端 /ws 正确令牌完成 101", st == 101, f"status={st}")
        if sock is not None:
            frames = ws_collect(sock, seconds=25.0,
                                predicate=lambda d: d.get("type") == "vehicle")
            veh = [d for d in frames if d.get("type") == "vehicle"]
            check("S5 远端授权连接收到 vehicle 帧流", len(veh) >= 1,
                  f"frames={len(veh)}")
            if veh:
                d = veh[-1]
                check("S5 vehicle 帧具备 §60 契约字段",
                      all(k in d for k in ("state", "position", "speed_kmh", "glosa")),
                      f"keys={sorted(d.keys())[:8]}")
            sock.close()
        else:
            check("S5 远端授权连接收到 vehicle 帧流", False, "101 后未取得 socket")

    # 回环 WS 同样要求令牌（Batch 3.5 修正：原先「LAN 模式回环豁免」）
    st, _, _ = ws_attempt("127.0.0.1", LAN_PORT, "/ws")
    check("S5 回环 /ws 无令牌不升级（401，回环 ≠ 已认证）", st == 401, f"status={st}")
    st, _, _ = ws_attempt("127.0.0.1", LAN_PORT, "/ws?token=" + "b" * 64)
    check("S5 回环 /ws 错令牌不升级（401）", st == 401, f"status={st}")
    st, _, s2 = ws_attempt("127.0.0.1", LAN_PORT, "/ws?token=" + TOKEN_LIVE)
    check("S5 回环 /ws 正确令牌完成 101", st == 101, f"status={st}")
    if s2:
        s2.close()
    # WS 也接受 Authorization 头（原生客户端路径）
    st, _, s3 = ws_attempt("127.0.0.1", LAN_PORT, "/ws", origin=NO_ORIGIN)
    check("S5 无 Origin 且无令牌的原生客户端仍被拒（401）", st == 401, f"status={st}")
    if s3:
        s3.close()

    # ── S6（续）旧令牌在新进程上失效 ───────────────────────────────────────
    if LAN_IP is None:
        not_verified("S6 旧令牌在新进程上 401", "无私网地址")
    else:
        r = http(LAN_IP, LAN_PORT, "GET", "/api/snapshot", bearer(TOKEN_STALE))
        check("S6 旧令牌在新进程上 401", r is not None and r[0] == 401,
              f"status={r[0] if r else 'CONN-FAIL'}")
    st, _, _ = ws_attempt("127.0.0.1" if LAN_IP is None else LAN_IP, LAN_PORT,
                          "/ws?token=" + TOKEN_STALE)
    check("S6 旧令牌不能升级 WS", st == 401, f"status={st}")

    # ── S7 metadata 隐私 ──────────────────────────────────────────────────
    # Batch 3.5：回环同样需要令牌，故统一带令牌访问（不再有「回环免令牌」分支）。
    host = LAN_IP or "127.0.0.1"
    hdrs = bearer(TOKEN_LIVE) if LAN_IP else bearer(DEFAULT_TOKEN or "")
    r = http(host, LAN_PORT, "GET", "/api/metadata", hdrs)
    body = r[2] if r else ""
    check("S7 metadata 可达", r is not None and r[0] == 200,
          f"status={r[0] if r else 'CONN-FAIL'}")
    leaks = [p for p in (":\\Users\\", ":\\Projects\\", "/home/", "\\\\", ":/")
             if p in body]
    check("S7 metadata 不含绝对路径片段", not leaks, f"命中={leaks} body={body[:160]}")
    try:
        mv = json.loads(body)
    except json.JSONDecodeError:
        mv = {}
    check("S7 metadata 不含 dataset_dir 原值",
          "dataset_dir" not in mv and "\\" not in body and "/" not in body,
          f"keys={sorted(mv.keys())}")
    check("S7 metadata 只提供非敏感元信息",
          set(mv.keys()) == {"nodes", "edges", "dataset"}
          and isinstance(mv.get("nodes"), int) and isinstance(mv.get("edges"), int)
          and isinstance(mv.get("dataset"), str) and ":" not in mv.get("dataset", ":"),
          f"keys={sorted(mv.keys())} dataset={mv.get('dataset')!r}")
    check("S7 metadata 的 dataset 为目录名而非路径",
          mv.get("dataset") == DATASET_DIR_HINT, f"got={mv.get('dataset')!r}")

    # 其它动态端点也不得夹带令牌
    for path in ("/api/settings", "/api/snapshot"):
        r = http(host, LAN_PORT, "GET", path, hdrs)
        check(f"S7 {path} 不回显令牌", r is not None and TOKEN_LIVE not in (r[2] or ""),
              f"status={r[0] if r else 'CONN-FAIL'}")

    # ── S8 CORS ───────────────────────────────────────────────────────────
    def acao(resp):
        return (resp[1].get("access-control-allow-origin") if resp else None)

    r = http("127.0.0.1", LAN_PORT, "GET", "/api/snapshot", bearer(TOKEN_LIVE))
    check("S8 同源（无 Origin 头）不返回 ACAO", r is not None and acao(r) is None,
          f"ACAO={acao(r)!r}")
    check("S8 同源请求本身成功（不依赖 CORS 头）", r is not None and r[0] == 200,
          f"status={r[0] if r else 'CONN-FAIL'}")

    r = http("127.0.0.1", LAN_PORT, "GET", "/api/snapshot",
             {"Origin": "http://tauri.localhost", **bearer(TOKEN_LIVE)})
    check("S8 已实测 Tauri origin 获得精确 ACAO",
          acao(r) == "http://tauri.localhost", f"ACAO={acao(r)!r}")
    check("S8 白名单响应带 Vary: Origin",
          r is not None and r[1].get("vary") == "Origin",
          f"Vary={r[1].get('vary') if r else None!r}")

    for bad in ("http://evil.example", "null", "http://tauri.localhost.evil.example",
                "tauri://localhost", "https://tauri.localhost"):
        r = http("127.0.0.1", LAN_PORT, "GET", "/api/snapshot",
                 {"Origin": bad, **bearer(TOKEN_LIVE)})
        check(f"S8 未批准 origin 无 CORS 头: {bad}", acao(r) is None, f"ACAO={acao(r)!r}")

    # 任何响应都不得出现通配符
    wild = []
    for origin in (None, "http://tauri.localhost", "http://evil.example"):
        hh = dict(bearer(TOKEN_LIVE))
        if origin:
            hh["Origin"] = origin
        for path in ("/api/snapshot", "/api/metadata", "/api/bootstrap", "/index.html"):
            rr = http("127.0.0.1", LAN_PORT, "GET", path, hh)
            if rr and has_cors_wildcard(rr[1]):
                wild.append((origin, path, rr[1].get("access-control-allow-origin")))
    check("S8 全端点均无 Access-Control-Allow-Origin: *", not wild, f"命中={wild}")

    # OPTIONS 预检
    r = http("127.0.0.1", LAN_PORT, "OPTIONS", "/api/route",
             {"Origin": "http://tauri.localhost",
              "Access-Control-Request-Method": "POST",
              "Access-Control-Request-Headers": "authorization,content-type"})
    check("S8 白名单 origin 的 OPTIONS 预检许可",
          r is not None and r[0] in (200, 204) and acao(r) == "http://tauri.localhost",
          f"status={r[0] if r else 'CONN-FAIL'} ACAO={acao(r)!r}")
    check("S8 预检 Allow-Methods 逐项列举且无通配",
          r is not None and r[1].get("access-control-allow-methods") == "GET, POST, OPTIONS",
          f"ACAM={r[1].get('access-control-allow-methods') if r else None!r}")
    check("S8 预检 Allow-Headers 含 Authorization 且无通配",
          r is not None
          and "Authorization" in (r[1].get("access-control-allow-headers") or "")
          and "*" not in (r[1].get("access-control-allow-headers") or ""),
          f"ACAH={r[1].get('access-control-allow-headers') if r else None!r}")

    r = http("127.0.0.1", LAN_PORT, "OPTIONS", "/api/route",
             {"Origin": "http://evil.example",
              "Access-Control-Request-Method": "POST"})
    check("S8 未批准 origin 的预检不返回任何 CORS 许可头",
          r is not None and acao(r) is None
          and r[1].get("access-control-allow-methods") is None,
          f"status={r[0] if r else 'CONN-FAIL'} headers={r[1] if r else None}")

    # ── 威胁模型边界：静态资源不鉴权（明确记录，不冒充全站鉴权）─────────────
    if LAN_IP:
        for path in ("/", "/index.html", "/app.js", "/style.css", "/manifest.json"):
            r = http(LAN_IP, LAN_PORT, "GET", path)
            check(f"静态资源免令牌可读（记录性断言）: {path}",
                  r is not None and r[0] == 200, f"status={r[0] if r else 'CONN-FAIL'}")
        # 静态资源不得夹带令牌
        r = http(LAN_IP, LAN_PORT, "GET", "/index.html")
        check("静态 HTML 不含令牌", r is not None and TOKEN_LIVE not in (r[2] or ""))
        r = http(LAN_IP, LAN_PORT, "GET", "/app.js")
        check("静态 JS 不含令牌", r is not None and TOKEN_LIVE not in (r[2] or ""))

    # ── S9 不受允许来源（Disallowed）在连接层被拒绝 ────────────────────────
    reachable = None
    for cand in DISALLOWED_CANDIDATES:
        probe = http(cand, LAN_PORT, "GET", "/api/snapshot", timeout=3.0)
        if probe is not None:
            reachable = cand
            break
    if reachable is None:
        not_verified(
            "S9 Disallowed 来源在连接层被拒绝（403）",
            f"本机无非 RFC1918 的可连接地址（候选 {DISALLOWED_CANDIDATES or '无'}）；"
            "该分支仅由 Rust 单元测试 classify_peer 覆盖",
        )
    else:
        print(f"（Disallowed 来源实测地址: {reachable}）")
        r = http(reachable, LAN_PORT, "GET", "/api/snapshot")
        check("S9 Disallowed 来源 GET /api/snapshot 被拒（403）",
              r is not None and r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
        r = http(reachable, LAN_PORT, "GET", "/api/snapshot", bearer(TOKEN_LIVE))
        check("S9 Disallowed 来源即使携带正确令牌仍被拒（403，非鉴权路径）",
              r is not None and r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
        r = post_route(reachable, LAN_PORT, bearer(TOKEN_LIVE), ROUTE_A)
        check("S9 Disallowed 来源 POST /api/route 被拒",
              r is not None and r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
        st, _, _ = ws_attempt(reachable, LAN_PORT, "/ws?token=" + TOKEN_LIVE)
        check("S9 Disallowed 来源 WS 即使携带正确令牌也不升级",
              st == 403, f"status={st}")
        r = http(reachable, DEFAULT_PORT, "GET", "/api/snapshot")
        check("S9 默认模式下 Disallowed 来源同样不可达",
              r is None or r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
        r = http(reachable, LAN_PORT, "GET", "/api/bootstrap")
        check("S9 Disallowed 来源 bootstrap 被拒",
              r is not None and r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
        check("S9 Disallowed 拒绝响应不泄露令牌",
              r is not None and TOKEN_LIVE not in r[2])

    # ── S10 Host 策略（§14）──────────────────────────────────────────────
    # 攻击场景：DNS rebinding 让攻击者的域名解析到 127.0.0.1，于是浏览器把请求发到
    # 本机服务，但 Host（与 Origin）都是攻击者的域名。服务端若以「Origin == Host」
    # 判同源，就会把这套自洽的组合当成合法来源。本组断言锁定：接受集合只来自
    # 服务器自身状态（回环名 + 本机实际地址 + 实际监听端口）。
    r = http("127.0.0.1", LAN_PORT, "GET", "/api/snapshot",
             {"Host": "evil.example", **bearer(TOKEN_LIVE)})
    check("S10 外部域名的 Host 被拒（DNS rebinding）",
          r is not None and r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
    r = http("127.0.0.1", LAN_PORT, "GET", "/api/snapshot",
             {"Host": "evil.example", "Origin": "http://evil.example", **bearer(TOKEN_LIVE)})
    check("S10 Host 与 Origin 同为攻击者域名仍被拒（不得自洽放行）",
          r is not None and r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
    r = http("127.0.0.1", LAN_PORT, "GET", "/api/snapshot",
             {"Host": f"127.0.0.1:{LAN_PORT + 1}", **bearer(TOKEN_LIVE)})
    check("S10 端口不符的 Host 被拒",
          r is not None and r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
    r = http("127.0.0.1", LAN_PORT, "GET", "/api/bootstrap", {"Host": "evil.example"})
    check("S10 bootstrap 同样受 Host 策略约束（不可绕过）",
          r is not None and r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
    st, _, _ = ws_attempt("127.0.0.1", LAN_PORT, "/ws?token=" + TOKEN_LIVE,
                          host_header=f"127.0.0.1:{LAN_PORT + 1}")
    check("S10 WS 握手同样受 Host 策略约束",
          st == 403, f"status={st}")
    # 正向：合法 authority 不得被误伤
    for ok_host in (f"127.0.0.1:{LAN_PORT}", f"localhost:{LAN_PORT}"):
        r = http("127.0.0.1", LAN_PORT, "GET", "/api/snapshot",
                 {"Host": ok_host, **bearer(TOKEN_LIVE)})
        check(f"S10 合法 authority 不被误伤: {ok_host}",
              r is not None and r[0] == 200, f"status={r[0] if r else 'CONN-FAIL'}")
    if LAN_IP:
        r = http(LAN_IP, LAN_PORT, "GET", "/api/snapshot",
                 {"Host": f"{LAN_IP}:{LAN_PORT}", **bearer(TOKEN_LIVE)})
        check(f"S10 本机实际私网 authority 不被误伤: {LAN_IP}",
              r is not None and r[0] == 200, f"status={r[0] if r else 'CONN-FAIL'}")

    # ── S11 浏览器 Origin 策略（bootstrap 与 WS 共用）─────────────────────
    # 攻击页面与 nav-server 同机不同端口 = 跨源。它可以让浏览器以**回环对端**身份
    # 发出请求，因此仅凭对端类别无法拒绝——这正是 Batch 3.5 的漏洞形态。
    attacker_origin = f"http://127.0.0.1:{LAN_PORT + 1}"
    r = http("127.0.0.1", LAN_PORT, "GET", "/api/bootstrap",
             {"Origin": attacker_origin})
    check("S11 回环对端 + 跨源 Origin 的 bootstrap 被拒（403）",
          r is not None and r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
    check("S11 该拒绝响应不含令牌",
          r is not None and TOKEN_LIVE not in (r[2] or ""))
    for bad in ("http://evil.example", "null", "https://127.0.0.1:" + str(LAN_PORT)):
        r = http("127.0.0.1", LAN_PORT, "GET", "/api/bootstrap", {"Origin": bad})
        check(f"S11 bootstrap 拒绝未批准 Origin: {bad}",
              r is not None and r[0] == 403 and TOKEN_LIVE not in (r[2] or ""),
              f"status={r[0] if r else 'CONN-FAIL'}")
    # 正向：同源页面与实测 Tauri origin 必须仍能取得令牌
    for ok in (f"http://127.0.0.1:{LAN_PORT}", "http://tauri.localhost"):
        r = http("127.0.0.1", LAN_PORT, "GET", "/api/bootstrap", {"Origin": ok})
        got = ""
        if r and r[0] == 200:
            try:
                got = json.loads(r[2]).get("token", "")
            except json.JSONDecodeError:
                got = ""
        check(f"S11 已批准 Origin 可取得令牌: {ok}",
              r is not None and r[0] == 200 and got == TOKEN_LIVE,
              f"status={r[0] if r else 'CONN-FAIL'}")

    # WS：持有**正确令牌**但 Origin 恶意，仍必须被拒——否则 Origin 检查等于被令牌
    # 完全覆盖而形同虚设（BLS-04 的服务端对应断言）。
    for bad in (attacker_origin, "http://evil.example", "null"):
        st, _, _ = ws_attempt("127.0.0.1", LAN_PORT, "/ws?token=" + TOKEN_LIVE,
                              origin=bad)
        check(f"S11 正确令牌 + 恶意 Origin 的 WS 仍被拒（403，无 101）: {bad}",
              st == 403, f"status={st}")
    # 正向：同源 Origin 与实测 Tauri origin
    for ok in (f"http://127.0.0.1:{LAN_PORT}", "http://tauri.localhost"):
        st, _, s = ws_attempt("127.0.0.1", LAN_PORT, "/ws?token=" + TOKEN_LIVE,
                              origin=ok)
        check(f"S11 已批准 Origin 的 WS 完成 101: {ok}", st == 101, f"status={st}")
        if s:
            s.close()
    # 原生客户端：不带 Origin，凭令牌升级（Origin 检查不得误伤原生客户端）
    st, _, s = ws_attempt("127.0.0.1", LAN_PORT, "/ws?token=" + TOKEN_LIVE,
                          origin=NO_ORIGIN)
    check("S11 无 Origin 的原生客户端凭令牌完成 101", st == 101, f"status={st}")
    if s:
        frames = ws_collect(s, seconds=25.0,
                            predicate=lambda d: d.get("type") == "vehicle")
        check("S11 原生客户端确实收到 vehicle 帧流",
              any(d.get("type") == "vehicle" for d in frames), f"frames={len(frames)}")
        s.close()

    print()
    print("=" * 72)
    print(f"FAIL={len(FAIL)}  NOT_VERIFIED={len(NOT_VERIFIED)}  SKIP={len(SKIP)}")
    if FAIL:
        for n in FAIL:
            print(f"  FAIL: {n}")
    if NOT_VERIFIED:
        for n in NOT_VERIFIED:
            print(f"  NOT VERIFIED: {n}")
    if SKIP:
        for n in SKIP:
            print(f"  SKIP: {n}")
    if FAIL:
        print("LAN SECURITY: FAIL")
        return 1
    if NOT_VERIFIED:
        print("LAN SECURITY: NOT VERIFIED（存在未执行的关键检查，不计为通过）")
        return 3
    print("LAN SECURITY: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
