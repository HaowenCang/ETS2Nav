#!/usr/bin/env python3
"""nav-server LAN 暴露面 / 鉴权 / CORS 集成验证（P4R Batch 3 §20 S1–S8）。

这不是单元测试：全部断言都打在**真实运行的 nav-core-cli server 进程**上，经真实
TCP/HTTP/WebSocket 连接，并且刻意区分两种来源：

    127.0.0.1        -> Loopback     （服务端豁免令牌）
    <本机 RFC1918>   -> PrivateLan   （服务端要求令牌）

关键在于：从本机连到自己的私网地址时，内核选用的源地址就是该私网地址，因此服务端
看到的对端**确实**是 PrivateLan，而不是回环。这使单机也能真实执行「远端来源」矩阵，
无需第二台设备，也不是靠桩件伪造来源。

用法:
    verify-lan-security.py <config.json>
    verify-lan-security.py --selftest

config.json 由 scripts/run-security-test.mjs 生成：
    {"defaultPort":N, "lanPort":N, "tokenLive":"...", "tokenStale":"...",
     "candidates":["10.x.x.x", ...]}

退出码：0 全部通过；1 存在 FAIL；2 自检失败；3 远端来源矩阵无法执行（NOT VERIFIED，
不计为通过——安全结论不能建立在未执行的检查上）。
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
TOKEN_LIVE = CFG["tokenLive"]
TOKEN_STALE = CFG["tokenStale"]
CANDIDATES = CFG.get("candidates") or []
LAN_IP = CANDIDATES[0] if CANDIDATES else None
DATASET_DIR_HINT = CFG.get("datasetDirHint", "europe-v5")
# 非 RFC1918 的本机地址（VPN/隧道/链路本地），用于真实执行 Disallowed 来源断言
DISALLOWED_CANDIDATES = CFG.get("disallowedCandidates") or []


# ─── 原始 HTTP / WS 客户端 ───────────────────────────────────────────────────


def http(host, port, method, path, headers=None, body=b"", timeout=8.0):
    """发一个 HTTP 请求。返回 (status, headers, body_text)；连接失败返回 None。"""
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


def ws_attempt(host, port, path, timeout=8.0):
    """尝试 WS 握手。返回 (status, headers, sock)；握手完成则 sock 可直接读帧。"""
    try:
        s = socket.create_connection((host, port), timeout=timeout)
    except OSError:
        return (None, {}, None)
    key = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nOrigin: http://{host}:{port}\r\n"
        f"Upgrade: websocket\r\nConnection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    s.sendall(req.encode())
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


ROUTE_A = {"from": [-58456, 32832], "to": [-52925, 36510]}
ROUTE_B = {"from": [-58456, 32832], "to": [-57000, 34500]}


def post_route(host, port, headers, payload):
    return http(host, port, "POST", "/api/route", headers, json.dumps(payload).encode())


# ─── 前置：令牌轮换（S6）与候选地址 ─────────────────────────────────────────


def main():
    print("=" * 72)
    print("nav-server LAN 安全集成验证（S1–S8）")
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
    check("S6 令牌为 256-bit（64 位小写十六进制）",
          len(TOKEN_LIVE) == 64 and all(c in "0123456789abcdef" for c in TOKEN_LIVE),
          f"len={len(TOKEN_LIVE)}")

    # ── S1 默认 bind ───────────────────────────────────────────────────────
    r = http("127.0.0.1", DEFAULT_PORT, "GET", "/api/snapshot")
    check("S1 默认模式回环可达 /api/snapshot", r is not None and r[0] == 200,
          f"status={r[0] if r else 'CONN-FAIL'}")
    if LAN_IP is None:
        not_verified("S1 默认模式私网地址不可达", "本机无 RFC1918 候选地址，无法构造私网来源连接")
    else:
        r2 = http(LAN_IP, DEFAULT_PORT, "GET", "/api/snapshot")
        check("S1 默认模式私网地址不可达（只绑 127.0.0.1）", r2 is None,
              f"status={r2[0] if r2 else 'CONN-FAIL(期望)'}")
    # 默认模式不得生成/返回令牌
    r = http("127.0.0.1", DEFAULT_PORT, "GET", "/api/lan-bootstrap")
    body = r[2] if r else ""
    check("S1 默认模式 bootstrap 为 enabled:false",
          r is not None and r[0] == 200 and json.loads(body).get("enabled") is False,
          body[:120])
    check("S1 默认模式 bootstrap 不含任何令牌字段",
          '"token"' not in body and TOKEN_LIVE not in body)

    # ── S2 LAN bootstrap ───────────────────────────────────────────────────
    r = http("127.0.0.1", LAN_PORT, "GET", "/api/lan-bootstrap")
    boot = {}
    if r and r[0] == 200:
        try:
            boot = json.loads(r[2])
        except json.JSONDecodeError:
            boot = {}
    check("S2 回环 bootstrap 返回 enabled:true", boot.get("enabled") is True, str(boot)[:160])
    check("S2 回环 bootstrap 返回可用令牌",
          boot.get("token") == TOKEN_LIVE and len(TOKEN_LIVE) == 64)
    addrs = boot.get("addresses") or []
    check("S2 bootstrap 返回至少一个候选地址", len(addrs) >= 1, json.dumps(addrs, ensure_ascii=False))
    check("S2 候选地址均为 RFC1918 且非回环",
          bool(addrs) and all(a["address"] != "127.0.0.1" for a in addrs)
          and all(a["address"].startswith(("10.", "192.168."))
                  or (a["address"].startswith("172.") and 16 <= int(a["address"].split(".")[1]) <= 31)
                  for a in addrs))
    check("S2 首个候选被标为 preferred",
          bool(addrs) and addrs[0].get("preferred") is True
          and all(a.get("preferred") is False for a in addrs[1:]))

    if LAN_IP is None:
        not_verified("S2 远端 bootstrap 被拒（403）", "无私网地址")
        not_verified("S2 远端拒绝响应不泄露令牌", "无私网地址")
    else:
        r = http(LAN_IP, LAN_PORT, "GET", "/api/lan-bootstrap")
        check("S2 远端 bootstrap 被拒（403）", r is not None and r[0] == 403,
              f"status={r[0] if r else 'CONN-FAIL'}")
        check("S2 远端拒绝响应不泄露令牌",
              r is not None and TOKEN_LIVE not in r[2] and "token" not in r[2].lower())

    # ── S3 远端 API 鉴权 ───────────────────────────────────────────────────
    if LAN_IP is None:
        for n in ["S3 snapshot 无令牌 401", "S3 snapshot 错令牌 401",
                  "S3 snapshot 正确令牌 200", "S3 route 无令牌 401",
                  "S3 route 错令牌 401", "S3 route 正确令牌 正常处理"]:
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
        # 伪造 Origin 不构成授权（§15：Origin 不是 authentication）
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

    # ── S4 未授权 route 不得产生副作用 ─────────────────────────────────────
    # 观测通道：回环 WS 客户端（LAN 模式下豁免令牌）接收 map_state 广播。
    observer_status, _, obs = ws_attempt("127.0.0.1", LAN_PORT, "/ws")
    check("S4 观测通道建立（回环 WS 免令牌 101）", observer_status == 101,
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

    # ── S5 WebSocket 鉴权 ─────────────────────────────────────────────────
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

    # WS 也接受 Authorization 头（原生客户端路径），且 loopback 免令牌
    st, _, s2 = ws_attempt("127.0.0.1", LAN_PORT, "/ws")
    check("S5 回环 /ws 免令牌可连接（LAN 模式豁免）", st == 101, f"status={st}")
    if s2:
        s2.close()

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
    host = LAN_IP or "127.0.0.1"
    hdrs = bearer(TOKEN_LIVE) if LAN_IP else None
    r = http(host, LAN_PORT, "GET", "/api/metadata", hdrs)
    body = r[2] if r else ""
    check("S7 metadata 可达", r is not None and r[0] == 200, f"status={r[0] if r else 'CONN-FAIL'}")
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

    r = http("127.0.0.1", LAN_PORT, "GET", "/api/snapshot")
    check("S8 同源（无 Origin 头）不返回 ACAO", r is not None and acao(r) is None,
          f"ACAO={acao(r)!r}")
    check("S8 同源请求本身成功（不依赖 CORS 头）", r is not None and r[0] == 200,
          f"status={r[0] if r else 'CONN-FAIL'}")

    r = http("127.0.0.1", LAN_PORT, "GET", "/api/snapshot",
             {"Origin": "http://tauri.localhost"})
    check("S8 已实测 Tauri origin 获得精确 ACAO",
          acao(r) == "http://tauri.localhost", f"ACAO={acao(r)!r}")
    check("S8 白名单响应带 Vary: Origin",
          r is not None and r[1].get("vary") == "Origin",
          f"Vary={r[1].get('vary') if r else None!r}")

    for bad in ("http://evil.example", "null", "http://tauri.localhost.evil.example",
                "tauri://localhost", "https://tauri.localhost"):
        r = http("127.0.0.1", LAN_PORT, "GET", "/api/snapshot", {"Origin": bad})
        check(f"S8 未批准 origin 无 CORS 头: {bad}", acao(r) is None, f"ACAO={acao(r)!r}")

    # 任何响应都不得出现通配符
    wild = []
    for origin in (None, "http://tauri.localhost", "http://evil.example"):
        hh = {"Origin": origin} if origin else None
        for path in ("/api/snapshot", "/api/metadata", "/api/lan-bootstrap", "/index.html"):
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
    # 「不受允许」指既非回环也非 RFC1918。要真实执行这条断言，必须让服务端看到
    # 这样一个对端地址——伪造来源不可行，因此改用本机非私网接口地址（VPN 隧道、
    # 链路本地等）：连到这些地址时内核选用的源地址就是它本身，服务端看到的是
    # 真实的不受允许来源。具体哪些地址可连接依机器而定，故逐个探测。
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
        # bootstrap 亦不得对不受允许来源开放
        r = http(reachable, LAN_PORT, "GET", "/api/lan-bootstrap")
        check("S9 Disallowed 来源 bootstrap 被拒",
              r is not None and r[0] == 403, f"status={r[0] if r else 'CONN-FAIL'}")
        check("S9 Disallowed 拒绝响应不泄露令牌",
              r is not None and TOKEN_LIVE not in r[2])

    print()
    print("=" * 72)
    total = len(FAIL) + len(NOT_VERIFIED) + len(SKIP)
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
