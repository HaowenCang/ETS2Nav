# P4 关门报告（2026-08-12）

**阶段**：P4 正式 UI（v0.2 §56–61；PLAN-P3plus.md §2 A2）
**关门口径**：沿用 P2/P3 先例——机器可验证部分完成即关门（tag），实机项登记已知限制（D6 决策：实机测试延后）。
**交付主线**：`31474f6`（nav-server 第一块）、`d144e5f`（正式 UI + Desktop + 交互链验证）+ 审计修复链 `0ab13d8` / `c28e362` / `933c187`。

---

## §1 阶段目标与出口条件

PLAN.md §4：P4 = 「正式 UI（Browser → Desktop → LAN Mobile → Android/iOS，v0.2 §56–61）」，入口条件 P2/P3，出口验收「高德式信息结构 + 60 FPS 目标」。

PLAN-P3plus.md §2 A2 拆分六项：正式布局、Desktop（Tauri）、自动缩放与 Junction View、PC→移动端 API 定稿、LAN 连接与 PWA、60 FPS 目标验证。

## §2 出口条件对照

| # | 出口条件（v0.2 §56–61） | 状态 | 证据 |
|---|---|---|---|
| 1 | §56 高德式信息结构 | ✅ | `tools/ets2nav-web/`：速度/限速、状态 chip、下一转向（含环岛出口）、信号+GLOSA、提醒播报条、剩余路线+进度、目的地设置、设置面板 |
| 2 | §57 自动缩放 `Z=f(v,D,complexity)` | ✅ | 已实现并在 headless DOM 实测（速度 79 km/h / NAVIGATING / 9.4 km / ExitMotorway）；拖动暂停 follow + ◎ 恢复 |
| 3 | §59 pmtiles 地图渲染（坐标转换只在渲染层） | ✅ | MapLibre GL 4.7.1 本地化（vendor/，离线可用）；`map.pmtiles`（P1-12 产物）经 addSource 接入（修复后移入 `map.on('load')` 消除静默缺失）；ETS2 米制 → `lng=x/111320, lat=z/111320` 仅在渲染层 |
| 5 | §60 HTTP + WebSocket API 定稿 | ✅ | HTTP：`/api/metadata` `/api/route` `/api/snapshot` `/api/search` `/api/settings`；WS：单一全量 `vehicle` 快照（7 类事件折叠映射）+ `map_state` 事件；20 Hz 广播 |
| 6 | §61 LAN 监听 + 二维码 + PWA | ✅（真机验收延后） | server 监听 `0.0.0.0`；qrcodejs 本地化生成二维码；PWA manifest（standalone/theme-color） |
| 7 | Desktop（Tauri 2） | ✅（构建冒烟） | `desktop/` release 4.2 MB exe，WebView2 加载内嵌前端，启动 6 s 进程存活 |
| 8 | 交互链验证 | ✅ | `verify-ui-chain.py` 6 项 ALL PASS（route 200 / polyline 1500 pts / WS navigating / speed>0 / remaining 递减 / snapshot 200）；另补 session 级 off-route→rerouting 全链路断言 |
| 9 | 60 FPS 渲染目标 | ⚠️ 未测（登记） | headless 无法测真实渲染帧率；数据链路（20 Hz 帧 → DOM）已验证。**登记 B5 实机** |
| 10 | 移动端真机（扫码/渲染/断线重连） | ⚠️ 未测（登记） | 登记 B5（D6） |

## §3 交付物

| 项 | 位置 |
|---|---|
| nav-server | `nav-core/tools/nav-core-cli/src/server.rs` / `server_cli.rs`（HTTP + WS，零第三方依赖：RFC3174 SHA-1 手写、RFC6455 帧处理、掩码解码、ping/pong） |
| 正式 UI | `tools/ets2nav-web/`（index.html / app.js / style.css / vendor/ / manifest.json / map.pmtiles） |
| 交互链验证 | `tools/ets2nav-web/verify-ui-chain.py` |
| Desktop | `desktop/`（Tauri 2 最小壳 + 图标生成脚本） |
| 合成 trace | `nav-core-cli syntrace` 子命令（路线插值 5 m + 速度曲线） |
| 验证记录 | `docs/validation/p4-ui-2026-08.md` |

## §4 关键修复记录（审计闭环）

批 1 审计（a2-correctness / a2-api-contract，共 BLOCKER 0 / MAJOR 11）全部修复，复审均 0/0：

- **A2c-M1** `next_maneuver` 距离实时化（原为 planning 固定值，段全长不递减；改用 `tracker.distance_to_edge`，实测 271→142 m 单调递减）
- **A2c-M2** syntrace 运动学修复（位置由速度积分驱动；speed×dt 累计 == 位置弧长 6,356 m，总时长物理正确 327 s）
- **A2c-M3** 回放循环每轮重建 session（Arrived no-op 卡死修复）
- **A2c-M4** `pending_dest` 消费提取为共用闭包（实时模式原不消费——生产场景修复）
- **A2c-M5** 瓦片层 `addSource` 移入 `map.on('load')`（style 未加载抛错被吞 → 瓦片静默缺失）
- **A2c-M6** 广播写超时 2 s/连接（慢客户端不再冻结数据源管道）
- **A2a-M1** §60 HTTP 四端点补齐（`/api/search` POI 过滤 + `/api/settings`）
- **A2a-M2** WS 事件类型定稿（单一全量快照契约 + reminders 结构化 kind/severity + map_state 事件）
- **A2a-M3** 补 session 级偏航重规划全链路测试
- **A2a-M4** `--fake-signal` 测试钩子 + 信号/限速卡片 headless DOM 验证

另修复 `read_http_head` 同包丢失 bug（head 与 body 同包到达时 body 丢失，单测锁定）。

## §5 已知限制（登记）

1. **60 FPS 渲染测量**：仅有数据链路验证，无真实渲染帧率测量 —— B5 实机。
2. **移动端真机**：PWA/二维码就绪，扫码连接与渲染体验 —— B5 实机。
3. **Desktop 视觉确认**：构建冒烟通过，窗口内 UI 与 server 联动的视觉确认登记手动检查。
4. **LAN 无鉴权**：`0.0.0.0` 监听无 token —— §61 的 token 机制留待 B5 定稿。
5. **live 模式无 20 Hz 节流**（a2-api-contract 复审遗留 MINOR）：回放模式按 sim time 节流，实时模式直通。
6. ~~**§57 自动恢复缺失 + 速度项反相**~~ → **已修复（2026-08-12）**：规范 §57 明列「高速→显示较远范围」与「随后自动恢复」，原实现速度项为正系数（与规范相反）、自动恢复仅按钮路径。现速度项取负、按「暂停 ≥8 s 且车速 ≥5 km/h」自动恢复。见 p4-p6-hardening-2026-08.md §3。
7. **移动端 WS 默认地址 `127.0.0.1`**：真机需手动改为 PC 内网 IP（二维码已含 IP/端口）——B5 定稿。
8. ~~**UI 未按事件类型分发**~~ → **已修复（2026-08-12）**：改为按 `type` 分发并新增 `onMapState`。实测 `map_state` 携带 1263 点 polyline——修复前该推送因无 `state` 字段抛错被静默吞掉，**从未被 UI 渲染**。见 p4-p6-hardening-2026-08.md §4。
9. **server `--replay` 播完自动循环**：演示/验证行为，生产实时模式无此行为。
10. **合成 trace 终点 352 m 偏移**（a2-correctness 残余）：虚拟终点以边终点替代所致，未修复。

## §6 验证命令

```
cd E:\Projects\Pi\ETS2Nav
run-p3-tests.bat                 # 链内 P1 -> P2 -> P3 全部 ALL PASS（含 server 模块编译门）
cd nav-core && cargo fmt --check && cargo clippy --all-targets && cargo test
python tools\ets2nav-web\verify-ui-chain.py    # 需先启动 nav-core-cli server --replay
```

## §7 结论

P4 的 §56–§61 全部功能项已交付并通过离线（回放驱动 + headless DOM）验证；两项与真实渲染/真机相关的验收项（60 FPS 测量、移动端实测）按 D6 决策登记为已知限制，依赖 B5 实机会话。

**P4 关门（机器可验证部分完成 + 实机项登记）。**
