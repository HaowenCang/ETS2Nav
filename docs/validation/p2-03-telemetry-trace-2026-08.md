# P2-03 Telemetry/Trace 报告（2026-08-10）

**依据**：P2-navigation-core-plan.md §31–37（Telemetry Runtime/Snapshot/Staleness/Pause/
Teleport/Recorder/Replay）。
**产物**：nav-telemetry crate（共享内存读取 + 事件检测 + trace recorder/replayer）+
nav-core-cli live/replay 命令。

## 一、交付

### nav-telemetry（计划 §31-37）
- **SharedMemory**：Windows 命名共享内存读取（`Local\ETS2NavTelemetry`）——零依赖
  kernel32 FFI（CreateFileMappingW/MapViewOfFile）；布局按 scs-nav-bridge.cpp
  telemetry_state_t pack(1) 逐字段偏移解析（sequence@0 / placement@56=0x38 /
  speed@96=0x60 / speed_limit@100=0x64——与 speed-validator 核对一致）
- **TelemetrySnapshot**（§32 统一类型）：sequence/sim 时间/位置/四元数/speed/limit/
  fuel/job（8 字段任务信息）——其他模块不直接读共享内存
- **TelemetrySource**（§33）：staleness 检测（sequence 超时 → Stale——不推进进度/
  不重规划）+ 断线自动重连
- **EventDetector**（§35）：Teleport（单帧位移 >50m）/ SimTimeReset（sim 倒退>1s）/
  SequenceRestart（插件重启）/ PauseChanged / JobChanged
- **TraceRecorder / replay**（§36-37）：`.navtrace` JSONL（带墙钟时间戳），
  10-20Hz 可读格式

### nav-core-cli
- `live [trace.navtrace]`（§126）：实时遥测输出 + 事件打印 + 可选录制
- `replay <trace.navtrace>`（§125）：回放 + 事件检测回放

## 二、验证

- **单元测试 4 个**（总计 9 个全绿）：Teleport/Pause 检测、SimReset/SequenceRestart 检测、
  trace 往返一致、快照序列化往返
- **真实数据回放**：P0 挂机记录（2026-08-09，517 帧/258s）转 navtrace → replay PASS——
  位置轨迹 (-58456,5,32832) → (-59165,27,33382) 正确还原，0 误报事件
- cargo fmt / clippy（0 warnings）/ test 全绿

## 三、关键设计

- 共享内存读为顺序非原子读（10-20Hz 轮询，sequence 变化检测）；volatile 语义由
  轮询模式保证
- Trace 格式选 JSONL（可读性优先——调试工具；二进制压缩留 P2-19 性能包）
- 事件检测为单步判定；组合确认（如 teleport 需多帧）由 P2-17 Session 负责

## 四、下一步

- P2-04 前置验证（UK 方向 / 限速实测 / 环岛 corpus）
- P2-05 Spatial index（匹配候选查询）
- P2-06 Map Matching（消费 TelemetrySnapshot + CSR + spatial）
