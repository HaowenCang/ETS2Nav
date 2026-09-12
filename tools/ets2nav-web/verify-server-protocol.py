# -*- coding: utf-8 -*-
"""nav-server 协议集成测试（§60 / §61）。

定位（P4R Batch 2 更正）：本脚本是**协议集成测试**，不是 Browser E2E。
它以原始 socket 直接驱动 HTTP 与 RFC6455 WebSocket，断言的是**协议层**行为：
静态/API 路由、帧类型白名单、快照通道、剩余里程推进、提醒结构化。

它**不**启动浏览器、**不**执行 app.js、**不**触碰 DOM、**不**加载 MapLibre。
历史上本文档与若干验证报告以「UI 链验证」「交互链验证」「headless DOM」描述本
脚本，该描述超出其实际能力，已更正为「协议集成测试」。UI/DOM/地图断言由
Playwright 套件承担（tools/ets2nav-web/tests/e2e/）。

用法：
    python verify-server-protocol.py [port]
    python verify-server-protocol.py --selftest
前置：nav-core-cli server --replay <trace.navtrace> <dataset-dir> 已在运行。
"""
import json
import socket
import struct
import sys
import time

# 输出通道编码（P4R Batch 5 §31：远端首次运行的失败即由此暴露）。
# Windows 上 Python 的 stdout/stderr 编码取自 ANSI 代码页：中文 locale（本机 936）
# 能编码中文，英文 locale 的 CI runner 是 cp1252，于是下面 check() 打印含中文与
# 「—」的结论行时抛 UnicodeEncodeError，脚本以 exit 1 结束——判定逻辑本身没有问题，
# 失败的是输出通道的编码假设。在仓库内修掉，使脚本在任何 locale 下都可运行，
# 而不是只在流水线里设置 PYTHONIOENCODING 掩盖。
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

# 「剩余里程推进」判定的最小连续递减步数。取 5 而非 1：单步递减可能来自噪声或
# 单帧抖动，连续 5 步（20 Hz 下约 0.25 s）才构成真实推进证据。
MIN_DECREASING_RUN = 5

# replay 循环重启时 remaining 会从 ~0 跳回整条路线长度；跳升超过该容差即判定为
# 观测窗口边界（而非「里程倒退」）。轨迹内的正常抖动远小于 1 m。
# replay 循环重启时 remaining 会从 ~0 跳回整条路线长度（数百米到数公里）；帧间正常
# 变化（含小幅倒退抖动）远小于该量级。超过阈值的跳升判定为观测窗口边界。
WRAP_MIN_JUMP_M = 50.0

# 事件类型白名单（§60 契约：单一全量 vehicle 快照 + map_state 独立事件）。
KNOWN_TYPES = {"vehicle", "map_state"}

# 目的地（柏林合成轨迹起终点，与 nav-core-cli syntrace 生成的 trace 一致）。
ROUTE_FROM = [-58456, 32832]
ROUTE_TO = [-52925, 36510]


class RemainingRun:
    """remaining_m 序列的推进判定状态机（wrap 感知）。

    在线观测（observe_progress）与离线回归用例（--selftest）共用本实现：两套判据
    各自演化正是本批要防的假信心来源。
    """

    def __init__(self, min_run=MIN_DECREASING_RUN, wrap_jump=WRAP_MIN_JUMP_M):
        self.min_run = min_run
        self.wrap_jump = wrap_jump
        self.prev = None
        self.run = 0
        self.best = 0
        self.wraps = 0
        self.count = 0

    def push(self, v):
        """送入一帧 remaining_m（None = 未导航，中断当前窗口）。"""
        self.count += 1
        if v is None:
            self.prev = None
            self.run = 0
            return
        if self.prev is not None:
            if v < self.prev:
                self.run += 1
                self.best = max(self.best, self.run)
            elif v > self.prev + self.wrap_jump:
                self.wraps += 1  # replay 循环边界：跳升回整条路线长度
                self.run = 0
            else:
                self.run = 0  # 持平、小幅倒退或小幅上升：均不计入推进
        self.prev = v

    @property
    def ok(self):
        return self.best >= self.min_run


def analyze_remaining(samples, min_run=MIN_DECREASING_RUN, wrap_jump=WRAP_MIN_JUMP_M):
    """离线判定：返回 (best_run, wraps, ok)。语义见 RemainingRun。

    必须区分两种「递增」：replay 循环边界（正常，跳升回整条路线长度）与真实里程
    倒退（异常）。原实现在整个观测窗口上做布尔「曾经递减」判断，一旦窗口恰好跨过
    或落在循环边界附近就误报 FAIL（Batch 1 实测约 10 分钟后出现一次）。此处改为：
    只在**同一次观测窗口内**统计连续递减，遇到跳升即重建窗口，取历史最长连续递减
    段作为证据。

    ok 的判据是「存在一段至少 min_run 步的连续递减」——
      真实不递减（恒定/单调递增）→ 永不达标 → FAIL（未回退原断言语义）
      正常 replay wrap         → 边界仅中断当前段，不影响已积累的最长段

    wraps 仅为诊断量，不参与 ok 判定：某类异常（如每帧 +100 m 的单调倒退）在阈值
    上与连续循环边界不可区分，故 wraps 对大跳升序列不具诊断意义。
    """
    st = RemainingRun(min_run, wrap_jump)
    for v in samples:
        st.push(v)
    return st.best, st.wraps, st.ok


def selftest():
    """wraparound 判定的确定性回归用例（--selftest，不需要服务器）。

    want_wraps 为 None 表示该用例不对 wraps 作断言（见 analyze_remaining 文档）。
    """
    cases = [
        # (名称, 样本, 期望 ok, 期望 wraps)
        ("严格递减", [500.0 - i * 4.0 for i in range(40)], True, 0),
        ("递减中跨循环边界",
         [200.0 - i * 4.0 for i in range(30)] + [2000.0]
         + [2000.0 - i * 4.0 for i in range(30)], True, 1),
        ("恒定不推进", [123.0] * 40, False, 0),
        ("小幅递增（每帧 +3m）", [100.0 + i * 3.0 for i in range(40)], False, 0),
        ("大幅递增（每帧 +100m）", [500.0, 600.0, 700.0, 800.0] * 5, False, None),
        # 每段都短于 min_run：证明判据不是「出现过跳变就算通过」
        ("碎段递减（每段不足）", ([100.0, 96.0] + [1000.0]) * 6, False, 6),
        ("含未导航帧但有一段完整推进",
         [None, None] + [800.0 - i * 3.0 for i in range(20)] + [None, 900.0], True, 0),
        ("窗口内剩余里程恒为 None", [None] * 40, False, 0),
        ("无样本（服务器未回帧）", [], False, 0),
        ("单样本（不足以证明推进）", [500.0], False, 0),
    ]
    failed = []
    for name, samples, want_ok, want_wraps in cases:
        best, wraps, ok = analyze_remaining(samples)
        wraps_ok = want_wraps is None or wraps == want_wraps
        status = "PASS" if (ok == want_ok and wraps_ok) else "FAIL"
        expected = f"期望 ok={want_ok}" + ("" if want_wraps is None else f" wraps={want_wraps}")
        print(f"[{status}] selftest {name} — best_run={best} wraps={wraps} ok={ok} ({expected})")
        if status == "FAIL":
            failed.append(name)
    print()
    if failed:
        print(f"PROTOCOL SELFTEST FAIL: {failed}")
        return 1
    print("PROTOCOL SELFTEST PASS")
    return 0


if __name__ == "__main__" and "--selftest" in sys.argv:
    sys.exit(selftest())

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8123
FAIL = []

#: 会话令牌（P4R Batch 3.5）。动态 API 与 /ws 在两种模式下都要求它，因此本脚本
#: 先经 `/api/bootstrap` 引导——与浏览器页面走的是同一条路径，不设测试旁路。
TOKEN = None


def bootstrap_token(port):
    """经回环 `/api/bootstrap` 取得会话令牌。失败即返回 None（后续断言据此 FAIL）。"""
    s = socket.create_connection(("127.0.0.1", port))
    req = (
        f"GET /api/bootstrap HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n"
        f"Connection: close\r\n\r\n"
    )
    s.sendall(req.encode())
    s.settimeout(8)
    data = b""
    while True:
        try:
            chunk = s.recv(4096)
        except socket.timeout:
            break
        if not chunk:
            break
        data += chunk
    head, _, body = data.partition(b"\r\n\r\n")
    if b" 200 " not in head.split(b"\r\n")[0]:
        return None
    try:
        return json.loads(body.decode("utf-8")).get("token")
    except (UnicodeDecodeError, json.JSONDecodeError):
        return None


def bearer_headers(tok):
    return {"Authorization": f"Bearer {tok}"} if tok else {}


def ws_connect(port):
    """原生客户端握手：不发 Origin，令牌经 query 传递（浏览器 WS API 无法设头）。"""
    s = socket.create_connection(("127.0.0.1", port))
    import base64
    import os
    key = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET /ws?token={TOKEN} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n"
        f"Upgrade: websocket\r\n"
        f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    s.sendall(req.encode())
    s.settimeout(8)
    resp = b""
    while b"\r\n\r\n" not in resp:
        resp += s.recv(1024)
    assert b"101" in resp.split(b"\r\n")[0], f"握手失败: {resp[:80]}"
    return s


def recv_frame(ss):
    hdr = ss.recv(2)
    op = hdr[0] & 0x0F
    ln = hdr[1] & 0x7F
    if ln == 126:
        ln = struct.unpack(">H", ss.recv(2))[0]
    payload = b""
    while len(payload) < ln:
        payload += ss.recv(ln - len(payload))
    return op, payload


def http_post(port, path, body):
    s = socket.create_connection(("127.0.0.1", port))
    auth = f"Authorization: Bearer {TOKEN}\r\n" if TOKEN else ""
    req = (
        f"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n"
        f"{auth}Content-Length: {len(body)}\r\nConnection: close\r\n\r\n".encode() + body
    )
    s.sendall(req)
    s.settimeout(10)
    data = b""
    while True:
        try:
            chunk = s.recv(4096)
        except socket.timeout:
            break
        if not chunk:
            break
        data += chunk
    head, _, body_resp = data.partition(b"\r\n\r\n")
    return int(head.split()[1]), body_resp


def check(name, ok, detail=""):
    status = "PASS" if ok else "FAIL"
    print(f"[{status}] {name}" + (f" — {detail}" if detail else ""))
    if not ok:
        FAIL.append(name)


def observe_progress(ws, min_run=MIN_DECREASING_RUN, max_frames=900, max_seconds=180.0):
    """观测帧流，直到出现一段足够长的连续递减（成功即提前返回）或预算耗尽。

    为什么要"观测到证据为止"而不是固定窗口（P4R Batch 2 §8）：
    设目的地之后，tracker 需要若干帧才与 matcher 对齐，此间 remaining 保持不变；
    固定 60 帧窗口正好落在该启动相位内，会得到 best_run=1 的假失败（实测 58 样本
    best_run=1，而同一服务器在 800 帧窗口下 best_run=75、remaining 11312→10521 m）。
    判据本身未放宽——仍然是"必须出现连续 N 步递减"，只是不再用与 tracker 启动相位
    赛跑的固定窗口来采样。真正不递减的帧流会耗尽预算并返回 best_run 不足。

    判定逻辑复用 RemainingRun——与 --selftest 的回归用例是**同一份实现**，避免离线
    用例与在线判据漂移（两套实现正是本批要防的假信心来源）。
    """
    run = RemainingRun(min_run)
    nav = False
    spd = False
    map_state_pts = 0
    glosa_seen = {"present": False, "null": False, "valid": True}
    started = time.time()
    for _ in range(max_frames):
        if time.time() - started > max_seconds:
            break
        try:
            op, payload = recv_frame(ws)
        except socket.timeout:
            break
        if op != 1:
            continue
        d = json.loads(payload)
        t = d.get("type", "vehicle")
        seen_types.add(t)
        if t == "map_state" and isinstance(d.get("polyline"), list):
            map_state_pts = max(map_state_pts, len(d["polyline"]))
        if t != "vehicle":
            continue
        if d.get("state") == "navigating":
            nav = True
        if d.get("speed_kmh", 0) > 0:
            spd = True
        if "glosa" in d:
            g = d["glosa"]
            if g is None:
                glosa_seen["null"] = True
            elif isinstance(g, dict):
                glosa_seen["present"] = True
                lo, hi = g.get("min_kmh"), g.get("max_kmh")
                if not isinstance(lo, int) or not isinstance(hi, int) or lo > hi or lo < 0:
                    glosa_seen["valid"] = False
            else:
                glosa_seen["valid"] = False

        run.push(d.get("remaining_m"))
        if run.ok:
            break  # 证据已足，提前结束（避免无谓等待）
    return {
        "nav": nav, "spd": spd, "samples": run.count, "map_state_pts": map_state_pts,
        "glosa": glosa_seen, "best_run": run.best, "wraps": run.wraps,
    }


# 1) 静态文件与 API 冒烟
# 前置：取得会话令牌（Batch 3.5：两种模式下动态 API 与 /ws 都要求令牌，回环不豁免）。
TOKEN = bootstrap_token(PORT)
check("GET /api/bootstrap 返回会话令牌（回环）",
      isinstance(TOKEN, str) and len(TOKEN) == 64,
      f"token={'<64 hex>' if TOKEN else None}")

# 未授权请求必须被拒（同一条路径上的负向对照，避免「令牌没生效也照样 PASS」）
s = socket.create_connection(("127.0.0.1", PORT))
s.sendall(
    f"GET /api/snapshot HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\nConnection: close\r\n\r\n".encode()
)
s.settimeout(5)
_unauth = b""
while True:
    try:
        chunk = s.recv(4096)
    except socket.timeout:
        break
    if not chunk:
        break
    _unauth += chunk
check("GET /api/snapshot 无令牌 401（回环 ≠ 已认证）",
      int(_unauth.split(b" ")[1]) == 401,
      f"code={int(_unauth.split(b' ')[1]) if _unauth else 'NO-RESPONSE'}")

ROUTE_BODY = json.dumps({"from": ROUTE_FROM, "to": ROUTE_TO}).encode()
code, body = http_post(PORT, "/api/route", ROUTE_BODY)
check("POST /api/route 200", code == 200, f"code={code}")
route = json.loads(body)
check("route 有 polyline", len(route.get("polyline", [])) > 100,
      f"pts={len(route.get('polyline', []))}")

# 2) WS 连接 + 帧流（设目的地后 navigating + 剩余递减）
# 事件类型统计跨本节与 2.5 节累计——map_state 于设目的地时推送一次，
# 若只在本节「跳过非 vehicle 帧」，该事件会被消费后丢弃而观测不到。
seen_types = set()
map_state_polyline_pts = 0

ws = ws_connect(PORT)
time.sleep(1.0)
http_post(PORT, "/api/route", ROUTE_BODY)
obs = observe_progress(ws)

# replay 循环边界会重建 session 并丢失已消费的目的地（见 Batch 2 报告「新发现
# 问题」），因此未观测到 navigating 时重发一次目的地再观测。这是对**已登记演示模式
# 缺陷**的显式适配，不是对断言的放宽：重发后仍不 navigating 则测试依旧 FAIL。
if not obs["nav"]:
    print("[INFO] 首个窗口未出现 navigating（疑为 replay 循环边界重建 session），重发目的地再观测")
    http_post(PORT, "/api/route", ROUTE_BODY)
    obs2 = observe_progress(ws)
    obs = {
        "nav": obs["nav"] or obs2["nav"],
        "spd": obs["spd"] or obs2["spd"],
        "samples": obs["samples"] + obs2["samples"],        "map_state_pts": max(obs["map_state_pts"], obs2["map_state_pts"]),
        "glosa": {
            "present": obs["glosa"]["present"] or obs2["glosa"]["present"],
            "null": obs["glosa"]["null"] or obs2["glosa"]["null"],
            # valid 用 AND 合并：任一窗口出现非法区间即为非法
            "valid": obs["glosa"]["valid"] and obs2["glosa"]["valid"],
        },
        "best_run": max(obs["best_run"], obs2["best_run"]),
        "wraps": obs["wraps"] + obs2["wraps"],
    }

best_run = obs["best_run"]
wraps = obs["wraps"]
rem_ok = best_run >= MIN_DECREASING_RUN
map_state_polyline_pts = max(map_state_polyline_pts, obs["map_state_pts"])
glosa_seen = obs["glosa"]
check("WS 帧流 state=navigating", obs["nav"])
check("WS 帧流 speed>0", obs["spd"])
check(
    f"WS 帧流 remaining 递减（连续 ≥ {MIN_DECREASING_RUN} 步，wrap 感知）",
    rem_ok,
    f"best_run={best_run} wraps={wraps} samples={obs['samples']}",
)
check("vehicle 帧含 glosa 字段（契约：恒存在，无建议为 null）",
      glosa_seen["present"] or glosa_seen["null"],
      f"present={glosa_seen['present']} null={glosa_seen['null']}")
check("glosa 区间合法（int 且 0 ≤ min ≤ max）", glosa_seen["valid"])

# 2.5) 事件类型与提醒结构化（A2a-M2 审计项）
# 2026-08-12 扩展：事件类型白名单断言——UI 曾把非 vehicle 帧交给 onSnapshot 并抛错吞掉，
# 使约定的 map_state 事件实际从未被处理。此处从协议侧锁定「只出现已约定类型」；
# map_state 在浏览器中的处理路径由 Playwright 套件覆盖，本脚本不作 UI 断言。
seen_reminder_kind = None
for _ in range(60):
    try:
        op, payload = recv_frame(ws)
    except socket.timeout:
        break
    if op != 1:
        continue
    d = json.loads(payload)
    t = d.get("type", "vehicle")
    seen_types.add(t)
    if t == "map_state" and isinstance(d.get("polyline"), list):
        map_state_polyline_pts = max(map_state_polyline_pts, len(d["polyline"]))
    if d.get("reminders"):
        seen_reminder_kind = d["reminders"][0].get("kind")
        break
check("WS 事件类型含 vehicle", "vehicle" in seen_types)
check("WS 事件类型全部已约定（无未知类型）", seen_types <= KNOWN_TYPES,
      f"seen={sorted(seen_types)}")
check("WS 设目的地后收到 map_state（含 polyline）", map_state_polyline_pts > 0,
      f"pts={map_state_polyline_pts}")
if seen_reminder_kind:
    check("reminders 结构化（kind 字段）",
          seen_reminder_kind in ("speed_limit_change", "overspeed", "red_light",
                                 "green_imminent", "glosa"),
          f"kind={seen_reminder_kind}")
else:
    print("[SKIP] reminders 未触发（无信号场景正常）——kind 结构化由 server 单测/伪造信号路径覆盖")

# 3) 快照轮询通道
# 合法媒体类型但 body 非法 → 400（原实现只算不判，该断言此前实际缺失）
code, _ = http_post(PORT, "/api/route", b"{}")
check("POST /api/route 非法 JSON body 400", code == 400, f"code={code}")
s = socket.create_connection(("127.0.0.1", PORT))
s.sendall(
    f"GET /api/snapshot HTTP/1.1\r\nHost: 127.0.0.1:{PORT}\r\n"
    f"Authorization: Bearer {TOKEN}\r\nConnection: close\r\n\r\n".encode()
)
s.settimeout(5)
data = b""
while True:
    try:
        chunk = s.recv(4096)
    except socket.timeout:
        break
    if not chunk:
        break
    data += chunk
snap_code = int(data.split(b" ")[1])
check("GET /api/snapshot 200", snap_code == 200)

print()
if FAIL:
    print(f"SERVER PROTOCOL FAIL: {len(FAIL)} 项 — {FAIL}")
    sys.exit(1)
print("SERVER PROTOCOL PASS")
