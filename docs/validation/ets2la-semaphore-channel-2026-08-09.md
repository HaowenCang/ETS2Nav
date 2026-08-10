# ETS2LA 信号灯数据通道调研（2026-08-09）

状态：✅ 结论确定

## 发现

1. **ETS2LA 架构**：游戏插件（ets2_la_plugin.dll，闭源）在游戏进程内读取信号灯内存状态，通过共享内存 `Local\ETS2LASemaphore` 提供给外部主程序（ETS2LA 应用）。
2. **数据结构**（ETS2LA.Semaphores.cs 公开定义，48 字节 × 40 灯）：世界坐标、cx/cy、朝向四元数、type（信号灯/道闸）、**time_remaining（剩余时间）**、**state（OFF/RED/GREEN/ORANGETORED/ORANGETOGREEN/SLEEP）**、id。
3. **游戏内存验证**（memscan-probe，只读扫描 eurotrucks2.exe）：
   - 找到 `ETS2LASemaphore` 字符串（模块代码段）→ 插件已加载
   - **共享内存对象未创建** → 需 ETS2LA 主程序连接后才创建
   - 游戏内存中确认存在我们的 `Local\ETS2NavTelemetry`（自研插件 ✓）
4. **结论**：ETS2LA 通道可用性依赖第三方主程序（外部依赖，v0.2 原则避免）；其价值在于：
   - 证明"游戏内存含精确信号灯状态（state + time_remaining）"且内存读取工程可行（ETS2LA 持续维护）
   - 若安装 ETS2LA 主程序，可作 ground truth 对照验证我们的锚定+外推方案

## 对 ETS2Nav 的意义

- **主方案不受影响**：观测锚定 + 外推已实测 ≤0.5s（P0-B Go 判定），不依赖 ETS2LA。
- **对照价值**：ETS2LA time_remaining 可精确验证外推误差（替代人工按键）。
- **V1 候选路线**（需决策）：自研内存读取（逆向偏移、随版本维护）vs 依赖 ETS2LA 通道。
- **额外发现**：ETS2LA 还暴露 Camera/Navigation/Traffic/ParkedVehicles 等游戏内部数据通道——对 v0.2 §54 动态事件识别有潜在价值，同样受外部依赖约束。

## 工具

- `tools/ets2la-probe/Ets2laProbe`：读取 ETS2LASemaphore（需主程序运行）——勘误 2026-08-10：该工具现为进程内存 dump（dump-lamp），共享内存读取器已移至 SemaphoreScan
- `tools/ets2la-probe/MemScanProbe`：进程内存字符串扫描（只读探针，已验证可用）

## 更新（2026-08-10）

- ETS2LA 共享内存通道**暂不可用**：用户确认 ETS2LA 版本未更新、不支持当前 ETS2 版本；待更新后重试（Ets2laProbe 验证）。
- 期间自主路径已就绪：vision-anchor（低成本区域视觉锚定，60×60px@10Hz，编译通过）——不依赖 ETS2LA。
- 内存反查工具链已备：memscan-probe v2（float 值搜索 + 地址 dump），ETS2LA 通道恢复后即可用其数据定位游戏内部信号灯结构。
