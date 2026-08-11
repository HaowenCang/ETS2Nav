# P3 Driving Assistant 关门报告（2026-08-11）

**阶段**：P3（A1：P3 提醒模块核心逻辑——PLAN-P3plus.md §2 A1）
**tag**：v0.4.0-p3　**main**：见 git log（本报告后）
**关门口径**：P2 先例——机器可验证部分完成即关门（tag），实机项登记已知限制；
提醒模块默认关闭交付（D6，2026-08-11）。

---

## 一、出口条件对照（P3-driving-assistant-plan.md §1 十个工作包）

| 包 | 内容 | 状态 | 证据 |
|---|---|---|---|
| P3-00 | 计划 + 基线核查 | ✅ | 实证：RoadItem 无显式 speed_limit → **免 dataset v3** |
| P3-01 | 前方限速查询（§40） | ✅ | speed.rs：10 测试（审计 B1 修复：虚拟段独立聚合）；Berlin 断点 2 个（起点边限速正确并入）；Europe 实测 |
| P3-02 | 限速对照 diagnostic（§39）+ 超速（§41） | ✅ | reminder.rs：10 测试（±5 容差/≤50:+3/>50:+5） |
| P3-03 | 红灯减速（§36）+ 即将绿灯（§37） | ✅ | 10 测试（d_stop 模型/四条件组合） |
| P3-04 | GLOSA（§38） | ✅ | 6 测试（窗口∩限速∩加速；量化 5 km/h） |
| P3-05 | 测速摄像头验证（§42） | ✅ **No-Go** | camera-probe 五级扫描 0 命中（73,141 prefab/2,024 model/144,912 sign） |
| P3-06 | 事件流 + TTS（§48/§49） | ✅ | speak.rs：5 测试 + 中文语音文本 + SAPI smoke（3 voices） |
| P3-07 | 合成回放套件 | ✅ | run-p3-tests.bat **ALL PASS**（P2 回归全链 + cargo 门 + 冒烟） |
| P3-08 | 性能 + 关门 | ✅ | 限速查询 p99 **0.2µs**（目标 <10µs）；全部指标达标 |

**总测试**：workspace 93 全绿（--all-targets 实测口径：P2 49 → P3 +44，其中审计两轮修复净增 6——毛增 7 删 1）；fmt PASS；clippy 0。

## 二、性能数字（P3-08 bench，Europe v4 全图）

| 指标 | 实测 | 目标 | 判定 |
|---|---|---|---|
| 加载冷启动 | 327ms | — | — |
| 常驻内存 | 217MB | <500MB | ✅ |
| 路线 p99 | 0.378ms | <500ms | ✅ |
| 匹配 p99 | 0.012ms | <10ms | ✅ |
| **限速查询 p99** | **0.2µs** | **<10µs** | ✅ 超 50× |
| 进程工作集（实测） | 328MB（释放 junctions 后） | <500MB | ✅ |

## 三、本阶段实证结论（计划修正）

1. **免 dataset v3**：RoadItem 无显式 speed_limit 属性；限速全来自
   country×class×city 模型（每 edge 单值）；真实分段在相邻边之间——前方限速
   由查询层聚合（零 schema 变更、零数据重建）。
2. **movement 边限速 100% -1**：写入端未定义该语义——查询层继承前值（Berlin
   断点 24→3 实证；审计 B1 修复后起点边限速正确并入，实测断点 2 个——24 为
   旧模型按边计数，真实断点以 2 为准）。
3. **摄像头 No-Go**：speed_camera 编码在 1.60 Europe 无实例（sign.sii 定义存在
   但 0 引用）——按 §42 不实现"前方 500m 测速"，camera-probe 留作 DLC 更新重验。

## 四、已知限制（登记，B 侧补齐）

| 项 | 对应 B | 说明 |
|---|---|---|
| 限速一致率（telemetry vs map）实机闭合 | B2 T1 | 决策函数就绪，±5 容差待实测校准 |
| 信号 runtime 关联 VERIFIED 实机确认 | B2 T3 | 提醒/GLOSA 以合成信号验证；绿灯时长 G=15s 配置默认待校准 |
| 提醒实机验收（TTS 听感/误报/提前距离参数/30s 间隔） | B4 | speak.rs 参数均为默认设计值 |
| 摄像头功能 | — | No-Go 结论；DLC 更新后 camera-probe 重验 |
| 长 road 跨城市边界中点判定 | — | 已知近似 |

**交付形态**：提醒模块默认关闭（D6 决策）——P4 UI 接入时以配置开关启用。

## 五、资产

- 代码：nav-router speed/reminder/speak/session 四模块（+44 测试；审计闭环补：虚拟段 3、overspeed/防轰炸、distance_to_edge、§40 随位推进各 1）
- 工具：camera-probe（五级扫描）、run-p3-tests.bat（4 步套件）
- 报告：docs/validation/p3-01~p3-07-2026-08.md（7 份）+ p3-closeout-2026-08.md（P3-00 为计划文档 P3-driving-assistant-plan.md；P3-08 内容并入本报告）
- 计划：P3-driving-assistant-plan.md（实证修正版）

## 六、下一步（PLAN-P3plus.md §6：A2 P4 UI 先行）

A1 完成 → **A2（P4 正式 UI：Browser→Desktop→LAN Mobile）**与 A3（P5 OD corpus）
并行推进；A3 为唯一可自足出口阶段。
