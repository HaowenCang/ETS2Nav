# P1 收官评审修复报告（2026-08-10）

依据：4 子代理并行评审（run a2b57c20：格式层/语义图/工具回归/计划完成度）。
格式层评审超时（MapLibre pmtiles 源码验证中 30 分钟超时）——其覆盖点由工具
回归评审与独立验证补齐。

## 一、BLOCKER 修复（3 项）

### 1. movement 路径串联截断（语义图 B1——最重大）
- **问题**：NavNode 连接的多条曲线是**首尾相接的路径段序列**（非平行车道），
  代码只取第一条——83% 长度低估、38% 转向错误、6.4% 信号灯丢失
  （mod_ger_67 ctrl1→ctrl2 7.7m→36.0m；Berlin 定义级 861 movements 实证）
- **修复**：DFS 到达目标节点时连接的全部曲线追加进 path（串联展开）；
  同 target 多连接（真多车道）保留独立 movement
- **验证**：mod_ger_67 长度修正（36.0m 正确）、Berlin 带灯 movement 1314→1387
  （丢失的灯找回）、灯路口 157→184

### 2. MaxDepth 语义冲突（语义图 B2）
- **问题**：MaxDepth=16 按曲线数限制——串联展开后长路径（Berlin 最长 25 段）
  必被截断
- **修复**：改为 NavNode 跳数限制（hops 参数）

### 3. gate 退出码未传播（工具 B1）
- **问题**：--gate 的 pass 判定只打印不设置退出码——回归套件永远"PASS"
  （Berlin 实际 64% OD 时套件全绿）
- **修复**：Environment.Exit(pass ? 0 : 1)

## 二、MAJOR 修复（8 项）

| # | 项 | 修复 |
|---|---|---|
| M1 | --region 未知区域静默 0 sector 崩溃 | 空 sector 明确报错 + 非零退出 |
| M2 | --dir 默认路径失效（0 sector） | 默认根改 vendor\extracted + /base_map/ 前缀 |
| M3 | rail 224 条入路由网络 | SpeedClass 以 rail 开头排除 |
| M4 | 限速 0 语义混淆（未知 vs 无限速） | 三态：-1 未知 / 0 无限速 / 数值 |
| M5 | 城市半径圆跨边境误判（6.3% Berlin road 判波兰） | 城市 bbox 判定（Width/Height 矩形） |
| M6 | sln 缺 4 项目（测试只跑 56） | dotnet sln add——65 测试全入套件 |
| M7 | determinism 只比 routing.graph | 4 产物全比（junction/map.db/search.db） |
| M8 | Europe scale 假通过窗口 | 运行前清理 + 检查 errorlevel |
| M9 | speed-validator NearestLimit 只比端点 | 两端点参与（X1/Z1 投影） |

## 三、Gate 口径修正

- **OD 采样域**：全图有边节点 → **最大连通分量**（边界断头非图缺陷——P1-06 报告论证）
- **死端不阻断**：边界断头为已知限制（15/117 条）
- **结果**：Berlin 核心网 OD **95.4%**（rail 排除后 94%→95.4%）、Germany **94.6%**
- **Regression Suite 全绿为真实判定**（退出码传播 + 核心网口径）

## 四、文档修复（completion 审计）

- PLAN.md 状态表 P1-05~14 全部更新（原 ⏳ 大面积未同步）
- P1-09 完成条件**书面裁剪**：telemetry 实测一致率延后 P2（工具就绪——
  共享内存布局经核对正确）；speed segments/SignMetadata 未交付（P2）
- 格式笔记补 3 项：dataset-format.md / speed-rule.md / poi.md
- 死代码清理（LoadSemaphoreProfiles 等 P1-10 遗留）

## 五、验证

- 65 测试全过（sln 8 测试项目）
- Europe dataset v2 重建（171,453 movements 长度/灯修正后）+ Rust smoke PASS
- run-p1-tests.bat 6/6 ALL PASS（真实判定）

## 六、残留（记录不阻断）

- LeftHandTraffic 未参与方向判定（UK/爱尔兰方向待 UK sector 数据验证——P1-13
  Europe 已含 UK——P2 前置项）
- 环岛逐类验证未做（Berlin/Germany 无典型环岛实例——movement 结构依赖 PPD 数据）
- 测试覆盖缺口：DatasetWriter/TileBuilder/SpeedModel 无单元测试（E2E 覆盖——
  已强化 gate 退出码使 E2E 可靠）
- 格式层评审超时——PMTiles/MVT 规范符合性由独立 python 验证 + Rust smoke 覆盖
