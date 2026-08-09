# P0 阶段门评审记录（v0.2 §75）

**日期**：2026-08-10
**状态**：✅ **P0 门通过，关门**
**Tag**：`v0.1.0-p0`

## 四项验收逐条确认

### 1. Map：单城市路由图基本正确，无需人工修复（§75-1）

| 证据 | 结果 |
|---|---|
| A3 HashFS/Sector 解析 | ✅ 282 sector 与 TruckLib 逐项对照零差异（174735 items / 248410 nodes） |
| A5 ScsGraph 构建 | ✅ 柏林核心城区：道路节点 2426、主分量 87%；+缓冲 16 sector：6194 节点、主分量 80% |
| A7 随机 OD 测试 | ✅ 主分量内 200/200 OD 100% 可达（固定随机种子） |
| 方向一致性 | ✅ GraphValidator 零方向矛盾 |

**结论**：单城市 routing graph 无人工修复可用。边界小分量确认为解析截断效应（缓冲不足），非图构建缺陷。
**延后项**：A6 正式化检测器、A8 map-inspector/graph-debugger 工具 → P1 前置任务。

### 2. Signal：时钟域/相位锚定明确，倒计时精度 ≤1s（§75-2）

| 证据 | 结果 |
|---|---|
| TL-01 时钟域 | ✅ simulation_time 驱动，interval 秒 = 真实秒（1:1，7 间隔倍率 1.000±0.7%） |
| TL-02 相位锚定 | ✅ H2 加载锚定确认（sim 连续窗口内驶离返回相位跳变 5.9→27.5s） |
| Case B 外推 | ✅ 同窗口外推最大误差 0.483s ≤ 1s |
| B8 精确数据源 | ✅ semaphore-bridge v8（48B/灯布局反查定案、SEH 兜底、协作取消、越界修复） |
| 数据源可用性 | ✅ **ETS2LA 共存定案**：隔离测试确认 ets2la_plugin.dll 单文件激活数组（无主程序依赖） |

**结论**：Go（Case B）。倒计时方案：观测锚定 + 段长学习 + 外推，未锚定期间 STATE_ONLY。
**遗留风险**：ets2la_plugin.dll 闭源依赖（方案 C 已确认：先共存推进，逆向激活评估延后）。

### 3. Telemetry：数小时稳定运行（§75-3）

| 证据 | 结果 |
|---|---|
| 短时验证 | ✅ 全字段正常、无崩溃（B3） |
| 审核修复 | ✅ scs-nav-bridge v2（income u64、job 清零、微秒累计、双实例防护）；semaphore-bridge v8（BLOCKER 卸载竞态等） |
| 数小时挂机 | ✅ 用户实测通过（C1：挂机 2-4 小时无崩溃、数据连续） |
| 退出游戏 | ✅ 无崩溃（v8 卸载协作取消验证） |

### 4. Performance：无显著影响（§75-4）

| 指标 | baseline | withnav | 差异 | 验收 |
|---|---|---|---|---|
| 平均 FPS | 150.2 | 148.2 | **-1.3%** | ✅ ≤1-2% |
| 1% low | 94.2 | 94.9 | **+0.7%**（未回退） | ✅ ≤2% |

**方法**：PresentMon 2.5.1（`--process_name --timed 120`）+ analyze.sh（mawk 兼容管道）。

## 副产品与资产

- **自研插件**：scs-nav-bridge v2（SDK 1.14 遥测）、semaphore-bridge v8（游戏内内存读取，48B/灯）
- **工具链**：telemetry-dump、signal-lab/analyze、sem-probe、SemaphoreScan、MirrorScan、HookScan、perf-bench（run-bench.bat + analyze.sh）
- **格式笔记**：hashfs.md、semaphore-profile.md、telemetry-channels.md
- **验证文档**：b3/tl01/tl02/p0b/ets2la-semaphore/semaphore-bridge（2026-08-09/10 共 6 份）

## P1 启动条件

- [x] P0 门通过（本评审）
- [ ] P1 前置：A6 检测器、A8 工具（或与 P1 合并排程）
- [ ] TruckLib（GPL-2.0）复用评估：确认 or-later 条款或独立分发方案（GPL-3.0 兼容性）
