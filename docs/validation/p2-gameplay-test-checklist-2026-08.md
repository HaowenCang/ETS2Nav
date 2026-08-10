# P2 游戏内实测测试清单（2026-08）

**依据**：P2-navigation-core-plan.md §40-41（Speed Gate）、§103（UK 环岛方向）、
§104（驾驶 corpus）、§115（信号 runtime 关联）、§144/§146（CPU/FPS）、
§178（G15 Full Session——计划规定"P2 真正关门条件"）。
**状态**：P2 已关门（tag v0.3.0-p2）——本清单为**已知限制的游戏内实测补测项**，
工具链全部就绪，需玩家配合驾驶采集。完成全部测试后更新本文档状态并补充报告。

---

## 一、前置条件：插件安装

将以下 DLL 复制到游戏插件目录（如不存在则创建）：

```
目标目录: E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2\bin\win_x64\plugins\
  1. telemetry-plugin\scs-nav-bridge\scs-nav-bridge.dll      （遥测桥接器，trace 数据源）
  2. telemetry-plugin\semaphore-bridge\semaphore-bridge.dll  （信号灯读取，P2-16 runtime 数据源）
```

验证就绪：启动游戏后运行

```
nav-core-cli live
```

输出持续遥测帧（非"无法连接"）即为就绪。live 模式同时录制 trace（.navtrace），
录制文件路径在启动时打印。

## 二、测试项清单

### T1 Speed Gate 限速采集（§40-41）
| 项 | 内容 |
|---|---|
| 目的 | 验证导航限速模型（speed_limit/fallback/cap）与游戏实际限速一致 |
| 操作 | 依次驾驶：①城市道路（30-50km/h 区）②国道（70-90）③高速公路（不限速段）④德国不限速高速 |
| 采集 | speed-validator 工具（C#，`tools/speed-validator`）+ live trace（含 speed_limit 通道） |
| 验收 | 各路段导航限速与游戏 HUD 限速一致；未知限速（-1）路段 fallback 值合理 |
| 产出 | 限速对照表 + 报告 `docs/validation/p3-speed-gate-2026-08.md`（或 P3 包内） |

### T2 UK 环岛驾驶实测（§103）
| 项 | 内容 |
|---|---|
| 目的 | 验证 P2-04 LeftHandTraffic 镜像修复后 UK 环岛绕行方向（应顺时针） |
| 操作 | 英国（伦敦/曼彻斯特等）驾驶经过 1-2 个环岛，正常绕行并驶出 |
| 采集 | live trace（位置+航向轨迹）——环岛段的 movement 匹配与几何绕行方向 |
| 验收 | trace 轨迹的绕行方向与数据集 movement 几何一致（顺时针） |
| 产出 | 环岛方向验证记录（补充 p2-04 报告） |

### T3 信号灯 runtime 关联验证（§115）
| 项 | 内容 |
|---|---|
| 目的 | 验证 Signal Linker 的 runtime 关联达到 VERIFIED（位置+方向+id 匹配） |
| 操作 | 驾驶经过 1-2 个红绿灯路口（优先选有导航路线的），在路口附近停留/通行 |
| 采集 | live trace + semaphore 共享内存（`nav-core-cli session` 输出 upcoming_signal） |
| 验收 | 受控 movement 的灯组关联置信度 VERIFIED（S≥0.8 且 id 匹配），state/倒计时正确 |
| 产出 | 信号关联验证记录（补充 p2-16 报告） |

### T4 G15 Full Session（§178——P2 最大遗留）
| 项 | 内容 |
|---|---|
| 目的 | 完整导航会话在真实游戏输入下的端到端闭环（P2-17 目前为合成帧验证） |
| 操作 | ①设置目的地（POI/坐标）②沿导航驾驶 3-5 分钟 ③故意偏航一次 ④观察重规划 ⑤到达 |
| 采集 | live trace + `nav-core-cli session` 实时快照（状态机/进度/maneuver/信号） |
| 验收 | 状态机全程合理：Navigating 为主、偏航后 Suspected→Rerouting→Navigating（≈1s）、到达 Arrived；无 Error |
| 产出 | G15 验证报告（关闭 P2 最大遗留） |

### T5 CPU / 游戏 FPS 回退（§144/§146）
| 项 | 内容 |
|---|---|
| 目的 | 导航运行时资源占用：CPU 1-3%、游戏 FPS 回退 ≤1-2% |
| 操作 | 同一场景两轮驾驶：插件卸载 vs 加载（或导航开 vs 关），记录 FPS |
| 采集 | 游戏内 FPS 计数器 / 外部工具（MSI Afterburner 等） |
| 验收 | 导航运行时 CPU 增量 ≤1-3%；FPS 回退 ≤1-2% |
| 产出 | 性能对照记录（补充 p2-19 报告） |

### T6（可选）matcher 权重 calibration（§47）
| 项 | 内容 |
|---|---|
| 目的 | 用真实 quat 航向 trace 校准匹配权重/阈值（当前为默认值） |
| 操作 | 任意 10 分钟城市+高速驾驶（T1/T4 的 trace 可复用） |
| 采集 | trace 回放 `nav-core-cli match`——统计 HIGH/MEDIUM 占比与 lateral 分布 |
| 验收 | HIGH+MEDIUM ≥90%（当前 2Hz 占位 quat 口径 86.3%） |
| 产出 | 权重冻结建议（供 P3 前校准） |

## 三、操作流程（单次驾驶可覆盖多项）

1. 安装插件（见第一节），启动游戏
2. `nav-core-cli live` 开始录制（或自动录制）
3. 路线建议：**城市出发（T1 城市段 + T3 路口）→ 高速（T1 高速段 + T5 FPS）→
   英国（T2 环岛）→ 回程偏航一次（T4）**——单趟覆盖 T1-T5
4. 停止录制，交付 trace 文件路径（默认 Temp 目录，可指定）
5. 分析侧执行：match / session / speed 校验，产出各测试报告

## 四、验收汇总表（完成后更新）

| 测试 | 状态 | 结论 | 报告 |
|---|---|---|---|
| T1 Speed Gate | ⬜ | — | — |
| T2 UK 环岛方向 | ⬜ | — | — |
| T3 信号 runtime | ⬜ | — | — |
| T4 G15 Full Session | ⬜ | — | — |
| T5 CPU/FPS | ⬜ | — | — |
| T6 matcher calibration | ⬜ | — | — |

---

**数据规模建议**：单趟 20-30 分钟驾驶即可满足全部测试；trace 文件通常 <10MB。
**关联资产**：nav-core-cli（live/replay/match/session/bench）、speed-validator、
semaphore-bridge v8、scs-nav-bridge v8。
