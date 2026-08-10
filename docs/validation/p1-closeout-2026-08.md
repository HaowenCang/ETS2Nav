# P1 阶段关门报告（2026-08-10）

P1 Map Compiler 完整化——15 个工作包（P1-00~P1-14）全部完成并通过
4 子代理收官评审修复。**Tag：v0.2.0-p1**。

## 一、P1 出口条件对照（P1-map-compiler-plan.md §4/§139）

| 条件 | 状态 | 证据 |
|---|---|---|
| 一次命令生成 Navigation Dataset（7 产物） | ✅ | map-inspector --all-sectors --dataset + --tiles |
| 0 fatal | ✅ | Europe 1358 sector：failed_prefabs=0、Rust smoke PASS |
| 0 unresolved parser error | ✅ | 全欧洲 2,432 prefab 全加载；SII corpus 失败率 0.3%（vehicle/climate 非地图语义） |
| 不静默跳过 | ✅ | diagnostics.json 完整记录（失败列表为空） |
| 确定性构建 | ✅ | routing.graph SHA-256 两次构建一致（4 产物全比） |
| Rust 独立读取 | ✅ | dataset-reader-smoke PASS（零依赖） |
| Berlin/Germany Gate | ✅ | 核心网 OD Berlin 95.4% / Germany 94.6%（0 fatal/0 非法/0 逆行） |
| P1-14 one command 套件 | ✅ | run-p1-tests.bat 6/6 ALL PASS（真实退出码判定） |

## 二、交付物清单

### 生产代码（map-compiler/src，10 项目）
| 项目 | 内容 |
|---|---|
| ScsHashFs | SCS 哈希文件系统（目录表/文件读取） |
| ScsSii | SII parser（corpus 驱动）+ semaphore profile 解析 |
| ScsSector | sector .base/.aux 解析（34 种 item）+ token 解码 |
| ScsResource | IScsResourceProvider（HashFs/Directory/Overlay）+ GameInstall + 指纹 |
| ScsDefinitions | Definition Resolver（6 typed models） |
| ScsPrefab | PPD v0x19 解析 + movement 恢复（NavNode 图） |
| ScsGraph | RoadGraph + RoutingGraph（分类边/反向表） |
| ScsMapModel | SemanticMap + Builder + SpeedModel + PoiExtractor + DatasetWriter |
| ScsValidation | ValidationEngine（5 验证器） |
| ScsVectorTiles | MVT 编码 + PMTiles v3 写入 + TileBuilder |

### 工具（tools/）
map-inspector（15 命令）、graph-debugger、dataset-reader-smoke（Rust）、
speed-validator、telemetry-dump、signal-lab、perf-bench、vision-anchor

### 数据规模（全欧洲，游戏 1.60.1.7）
1358 sector / 253 万 items / 344 万 nodes / 143,677 roads（v2 rail 排除后）/ 42,843 junctions /
171,453 movements / 426,907 graph edges（v2）/ 4,997 POI / 2,432 prefab 种类
（**勘误 2026-08-10**：此数字为修复前口径——P1 构建漏负 x sector 422 个；
Europe v4 全量：281,012 movements / 696,717 edges / 7,918 POI——详见 p1-13 勘误表）

## 三、关键实测发现（P1 期间的工程知识沉淀）

1. **road 方向语义**：sector 存无前缀 road type（at1）→ 补 `road.` 前缀查定义；
   lanes_right-only → node0→node1（翻转实验 94%→2.4% 验证）
2. **movement 恢复**：NavCurve 端点 = NavNode 索引（非 ControlNode）；Input/OutputLines
   数据不全（mod_ger_67 实证）——正确语义是 NavNode 连接图（Physical=ControlNode）
3. **1.60 信号灯**：prefab item 的 SemaphoreProfile 字段几乎废弃（1/693）——
   灯配置全在 PPD 内部；signal group = PPD SemaphoreId
4. **sector 网格**：4096×4096（按城市坐标 bbox 实测）
5. **SII corpus**：.sui 裸 unit、行内 `{`、`//` 注释、并行数组、哈希 token 容错
6. **PPD 细节**：gzip 尾 Dispose 写入（单流连续写截断）、token 字符集数组化
   （C# `"\000…"` 转义歧义）

## 四、已知限制（延后 P2，均书面记录）

| 项 | 说明 | 记录位置 |
|---|---|---|
| P1-09 telemetry 实测一致率 | 工具就绪（共享内存布局核对正确），实测需游戏内运行 | p1-09-sign-camera-research.md |
| speed segments / SignMetadata | 未实现 | 同上 |
| LeftHandTraffic 方向 | UK/爱尔兰方向假设未验证（Europe 已含 UK） | p1-closing-review-fixes |
| 环岛逐类验证 | Berlin/Germany 无典型环岛实例 | p1-06 报告 |
| 转向类型近似 | TurnAngle 几何近似（P2 lane guidance 细化） | p1-05 |
| 公司 access 部分缺失 | Germany 5/80 prefab 节点无道路引用 | poi.md |
| 格式层评审超时 | PMTiles/MVT 由独立 python 验证覆盖 | p1-closing-review-fixes |

## 五、验证报告清单（docs/validation/）

p1-06（Berlin Gate）/ p1-07（Germany Scale）/ p1-08（POI）/
p1-10（Semaphore Binding）/ p1-11（Dataset Writer）/ p1-12（Vector Tiles）/
p1-13（Europe Build）/ p1-14（Regression Suite）/ p1-closing-review-fixes（收官评审修复）

## 六、版本

- Tag：**v0.2.0-p1**（P1 关门）
- 上一版本：v0.1.0-p0（P0 关门）
- P1 提交数：P1-03 起共 19 个提交（13 个工作包/修复提交 + 2 个补丁清理 + 2 个评审修复 + 2 个关门收尾——勘误 2026-08-10，原记 12+2；分支策略偏差：
  计划 15 PR vs 实际直提 main——记录于 p1-closing-review-fixes）

## 七、P2 建议起点

1. Routing 引擎（RoutingGraph 消费——边类型/长度/限速已就绪）
2. 驾驶引导（movement 转向/信号灯 group 已绑定）
3. P1-09 实测闭环（speed-validator 游戏内运行 → 限速一致率验收）
4. UK/Ireland 方向验证（LeftHandTraffic）
5. 信号灯联动（PPD SemaphoreId ↔ ETS2LA 灯数组桥接）
