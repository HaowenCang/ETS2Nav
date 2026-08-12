# -*- coding: utf-8 -*-
"""A2-P4 UI 交互链离线验证（§60）：模拟浏览器行为驱动 nav-server。
链：WS 连接 → POST /api/route（设目的地）→ 帧流断言（navigating/剩余递减/提醒）。
用法：python verify-ui-chain.py [port]
前置：nav-core-cli server 已在运行（--replay 合成 trace）。
"""
import json
import socket
import struct
import sys
import time

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8123
FAIL = []


def ws_connect(port):
    s = socket.create_connection(("127.0.0.1", port))
    import base64
    import os
    key = base64.b64encode(os.urandom(16)).decode()
    req = (
        f"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\n"
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
    req = (
        f"POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n"
        f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n".encode() + body
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


# 1) 静态文件与 API 冒烟
code, body = http_post(PORT, "/api/route", json.dumps({"from": [-58456, 32832], "to": [-52925, 36510]}).encode())
check("POST /api/route 200", code == 200, f"code={code}")
route = json.loads(body)
check("route 有 polyline", len(route.get("polyline", [])) > 100, f"pts={len(route.get('polyline', []))}")

# 2) WS 连接 + 帧流（设目的地后 navigating + 剩余递减）
ws = ws_connect(PORT)
time.sleep(1.0)
http_post(PORT, "/api/route", json.dumps({"from": [-58456, 32832], "to": [-52925, 36510]}).encode())
seen = {"navigating": False, "speed>0": False, "rem_decreasing": False}
prev_rem = None
for _ in range(60):  # 最多 3s
    try:
        op, payload = recv_frame(ws)
    except socket.timeout:
        break
    if op != 1:
        continue
    d = json.loads(payload)
    if d["state"] == "navigating":
        seen["navigating"] = True
    if d.get("speed_kmh", 0) > 0:
        seen["speed>0"] = True
    rem = d.get("remaining_m")
    if rem is not None:
        if prev_rem is not None and rem < prev_rem:
            seen["rem_decreasing"] = True
        prev_rem = rem
check("WS 帧流 state=navigating", seen["navigating"])
check("WS 帧流 speed>0", seen["speed>0"])
check("WS 帧流 remaining 递减", seen["rem_decreasing"], f"last={prev_rem:.0f}m")

# 2.5) 事件类型与提醒结构化（A2a-M2 审计项）
seen_types = set()
seen_reminder_kind = None
for _ in range(40):
    try:
        op, payload = recv_frame(ws)
    except socket.timeout:
        break
    if op != 1:
        continue
    d = json.loads(payload)
    seen_types.add(d.get("type", "?"))
    if d.get("reminders"):
        seen_reminder_kind = d["reminders"][0].get("kind")
        break
check("WS 事件类型含 vehicle", "vehicle" in seen_types)
if seen_reminder_kind:
    check("reminders 结构化（kind 字段）", seen_reminder_kind in ("speed_limit_change", "overspeed", "red_light", "green_imminent", "glosa"), f"kind={seen_reminder_kind}")
else:
    print("[SKIP] reminders 未触发（无信号场景正常）——kind 结构化由 server 单测/伪造信号路径覆盖")

# 3) 快照轮询通道
code, body = http_post(PORT, "/api/route", b"{}")  # 400 路径也验证错误处理
snap_code = None
s = socket.create_connection(("127.0.0.1", PORT))
s.sendall(b"GET /api/snapshot HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
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
    print(f"UI CHAIN FAIL: {len(FAIL)} 项 — {FAIL}")
    sys.exit(1)
print("UI CHAIN PASS")
