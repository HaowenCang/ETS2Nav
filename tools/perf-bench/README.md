# P0-D 性能基准测试流程（v0.2 §63）

## 目标

对照验证：ETS2 only（baseline）vs ETS2 + ETS2Nav 插件（withnav）的帧率影响。
验收标准（v0.2 §63）：平均 FPS 差异 ≤1~2%，1% low 回退 ≤2%。

## 工具

- PresentMon v2.5.1：控制台版本由 `run-bench.bat` 自动查找（可用环境变量
  `PRESENTMON` 指定完整路径，例如
  `%ProgramFiles%\Intel\PresentMon\PresentMonConsoleApplication\PresentMon-2.5.1-x64.exe`）。
  路径不再硬编码进脚本（P4R Batch 5 §36）。
- 记录脚本：`run-bench.bat`（自动提权，输出 CSV 到 `data\`）

## 操作步骤（约 15 分钟）

### 准备
1. 关闭游戏
2. 备份插件目录：把 `游戏\bin\win_x64\plugins\` 全部 DLL 移到 `plugins_backup\`（对照组干净）
   （注意：ETS2LA 相关 DLL 也移走——基准需要纯净对照；ETS2LA 主程序可保持关闭）

### 对照组 A（baseline）
3. 启动游戏 → 进入驾驶界面 → 到一段**固定高速路线**（建议选一段无红绿灯的长直高速，记录起点标记）
4. 右键管理员运行 `run-bench.bat baseline 120`
5. 立即在游戏内**沿固定路线巡航 120 秒**（保持速度稳定，如定速巡航 80-90 km/h）
6. 脚本自动结束 → 记录完成
7. 退出游戏

### 实验组 B（withnav）
8. 把 `plugins_backup\` 中的 DLL 恢复：仅放回 **scs-nav-bridge.dll + semaphore-bridge.dll**（其他插件不放，保持对照纯净）
9. 启动游戏 → 同一路线起点 → 管理员运行 `run-bench.bat withnav 120`
10. 同路线巡航 120 秒（尽量与 A 组一致：同速度/同天气/同 traffic）
11. 退出游戏

### 完成
12. 把 `tools\perf-bench\data\baseline.csv` 和 `withnav.csv` 路径发我 → 我分析（平均 FPS、1% low、CPU/GPU busy）

## 注意事项

- 两次测试尽量相同条件：同路线/同速度/同画质/同天气/同 traffic 密度
- 游戏内时间（昼夜）尽量一致（如都白天）
- PresentMon 需管理员权限（脚本已自动提权）
- 若有其他后台程序影响帧率，两次测试保持相同后台状态
