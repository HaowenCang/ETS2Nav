# P1-14 Regression Suite 验证报告（2026-08）

依据：P1-map-compiler-plan.md §107。命令：`run-p1-tests.bat`（需 ETS2_INSTALL 环境变量）。

## 结论

**通过**。one command → complete P1 test suite：6/6 全部 PASS。

## Suite 组成

| # | 项目 | 内容 | 结果 |
|---|---|---|---|
| 1 | 单元测试 | parser corpus（SII/Sector/HashFs）+ 格式解析 | PASS（65 测试——勘误 2026-08-10，原记 69） |
| 2 | Berlin Gate | semantic corpus + 500 deterministic OD | PASS |
| 3 | Germany Gate | scale + 多样性（39 sector） | PASS |
| 4 | Determinism | 两次 build 的 routing.graph/junction.graph/map.db/search.db 四个产物 SHA-256 全一致 | PASS |
| 5 | Rust dataset reader | 独立读取校验（边引用/自环/计数） | PASS |
| 6 | Europe scale | 全欧洲 build failed_prefabs = 0 | PASS |

## Determinism 验证

同一输入两次构建 routing.graph/junction.graph/map.db/search.db——四产物 SHA-256 完全一致（收官评审 M7 后为 4 产物比对；manifest 的 generated_at
时间戳除外——二进制图无时间依赖）。

## 鲁棒性修复

GameInstall.Detect 增加路径 Trim（防御环境变量尾随空格——cmd `set VAR=value &&`
会把 `&&` 前空格纳入值）。

## 通过条件（正式）

> one command → complete P1 test suite

✓ `run-p1-tests.bat`（ETS2_INSTALL 指向游戏目录）→ ALL PASS。

## P1 阶段收尾状态

P1-00~P1-14 全部工作包完成（P1-04/05/06/07 核心交付 + Gate 通过），
P1 出口条件满足。下一步：P2 阶段规划（Routing 引擎 / 驾驶引导 / 信号灯联动）。
