# P3 Driving Assistant 执行计划（A1）

> 依据：PLAN-P3plus.md §2 A1（P3 提醒模块核心逻辑）、v0.2 §35–42/§48–49、
> P2 关门先例（机器可验证完成即关门 + 实机项登记已知限制）。
> 执行模式（D6，2026-08-11）：实机测试延后——本计划全部工作包离线可验证；
> 提醒模块默认关闭交付，启用边界待 B2/B4。
> 分支策略：feat/p3-xx-* → main；每包报告 docs/validation/p3-xx-2026-08.md；
> cargo fmt --check / clippy 0 warnings / cargo test 绿。

---

## §0 范围与边界

**开发内容**（PLAN-P3plus.md A1 四项）：

1. 前方限速数据链（§40）：沿 route 的 speed-limit interval 查询（"前方 300 m 限速 50"）；
   Rust 热路径查询。
2. 提醒决策模块（§39/§41/§36/§37/§38）：当前限速对照 diagnostic、超速提醒
   （≤50: +3 / >50: +5 可配置）、红灯减速（d_stop = vt_r + v²/2a + d_m）、
   即将绿灯（§37 条件组合）、GLOSA（§38 窗口交集）。
3. TTS 语音播报（§48/§49）：Windows 离线 TTS + 播报频率管理。
4. 测速摄像头验证（§42）：全欧洲 camera position/controlled direction/speed limit
   覆盖率 → Go/No-Go 报告。

**排除**（登记已知限制，B 侧补齐）：
- 限速一致率实测闭合（B2 T1）——合成验证先行
- 信号 runtime 关联实测（B2 T3）——用合成信号事件验证决策模块
- 实机提醒验收（B4）
- TTS 实际听感体验（B4）

**数据集（实证修正，P3-00，2026-08-11 审计闭环回写）**：RoadItem 无显式
speed_limit 属性（SectorFile.cs 核查）——限速全部来自 SpeedModel（country ×
speed_class × IsCityRoad，每 road 单值）；edge == 单条 road，真实限速分段发生在
**相邻边之间**。因此**无需 dataset v3**：沿用 Europe v4（edge.speed_limit 已含
每边限速），前方限速由 Rust 侧沿路线前视聚合查询提供。零 schema 变更、零数据集
重建。

**实证结论（审计闭环回写，与 closeout §三 对齐，共 3 项）**：
1. 免 dataset v3（上述）；
2. **movement 边限速 100% -1**：写入端未定义该语义（routing 边 281,141 全 -1，
   Road 仅 1.8%=7,394）——查询层继承前值；审计 B1 修复后起点边限速正确并入，
   Berlin 实测断点 2 个（24 为旧模型按边计数，真实断点以 2 为准）；
3. **测速摄像头 No-Go**：五级扫描 0 实例（P3-05）——按 §42 不实现，probe 留作
   DLC 更新重验。

---

## §1 工作包拆分

| 包 | 内容 | 验证 |
|---|---|---|
| P3-00 | 本计划 + 现有代码基线核查（SpeedModel/DatasetWriter/SectorFile/nav-dataset） | 计划落盘（实证：无显式 speed_limit 属性 → 免 v3） |
| P3-01 | 前方限速查询（§40）：沿 route edge 序列聚合 (offset, limit) 断点（相邻同值去重、-1 未知/0 无限速如实上报、horizon 截断）+ 当前限速查询 | 单元测试（合成 route 断点断言）+ Europe 路线实测 |
| P3-02 | 限速对照 diagnostic（§39）：telemetry limit vs map limit 不一致 → diagnostic 事件 | 合成 trace 断言 |
| P3-03 | 超速提醒（§41 阈值可配置：≤50: +3 / >50: +5） | 合成 trace 断言 |
| P3-04 | 红灯减速（§36 d_stop 模型）+ 即将绿灯（§37 条件组合） | 合成信号序列断言 |
| P3-05 | GLOSA（§38 速度窗口 ∩ 限速 ∩ 加减速度 → 低精度区间） | §38 示例 53.6–64.2 → 50–60 数学验证 |
| P3-06 | 提醒事件流定稿（ReminderEvent）+ TTS（§48 语义事件 + §49 频率管理 D=f(v,class,complexity) + Windows SAPI 离线通道） | 事件流回放断言；TTS 通道 smoke（不发声验证） |
| P3-07 | 测速摄像头验证（§42）：camera item 提取 + 覆盖率统计 | Europe 覆盖率报告 → Go/No-Go |
| P3-08 | 合成回放验证套件：run-p3-tests.bat（P1/P2 回归 + 提醒断言 + 全量构建） | ALL PASS |
| P3-09 | 性能 + 关门：限速查询热路径 bench（p99 <10µs）、P3 关门报告、tag v0.4.0-p3 | 报告 + tag |

## §2 关键设计决策

**D1 限速数据源**（§55 裁剪实证）：country default × speed_class × IsCityRoad
（SpeedModel 现状）——road item 无显式 speed_limit 属性，sign/city rule/
map-specific 数据不足，不实现（登记）。单条 road 限速取中点判定（跨城市边界的
少数长 road 为已知近似）。

**D2 前方限速查询**：沿 route edge 序列聚合 (offset_m, limit) 断点，相邻同值去重，
horizon 截断；-1（未知）与 0（无限速）如实上报（不合并）；**非 Road 边
（JunctionMovement/Ferry/Train/ServiceAccess）继承前值**（P3-01 实证：写入端未
定义这些边类型的限速语义，routing 中 100% 为 -1，路口内部短连接不产生限速变化；
Road 自身 1.8% 未知维持 -1 如实上报）。零 schema 变更。

**D3 提醒事件流**：统一 ReminderEvent 枚举（SpeedLimit/OverSpeed/RedLight/
GreenImminent/Glosa——Camera 变体随 §42 No-Go 移除），与 §48 语义分离
（UI/TTS 消费端各自转换）；会话管线 session.rs 输出提醒流（NavigationSnapshot.
reminders，headless 可断言；2026-08-11 审计修复 A4 兑现）。

**D4 TTS 频率管理**：D=f(v, road_class, maneuver complexity) 计算播报提前距离
（§49 标准档），同类提醒最小间隔（§48 防轰炸）——配置化（低频/标准/高频）。

## §3 已知限制（关门时登记）

- 限速一致率（telemetry vs map）实机闭合 —— B2 T1
- 信号 runtime 关联 VERIFIED 实机确认 —— B2 T3（决策模块以合成事件验证）
- 提醒实机验收（含 TTS 听感、误报率）—— B4
- sign/city rule/map-specific 限速数据源 —— 不实现（数据不足，§55 裁剪）
- 长 road 跨城市边界的中点判定近似 —— 登记
- 摄像头若 No-Go —— "前方 500m 测速"不实现并记录结论

---

## §4 执行顺序

P3-00 → P3-01 → P3-02 → P3-03 → P3-04 → P3-05 → P3-06 → P3-07 → P3-08 → P3-09
（P3-02~05 决策模块按序实现；P3-07 可穿插）
