# P1-06 Berlin Semantic Gate 验证报告（2026-08）

依据：P1-map-compiler-plan.md §99。数据：游戏 1.60.1.7，柏林 8 sector（16 文件，
45189 items / 59903 nodes）。验证工具：`map-inspector --gate`。

## Gate 结论

**通过（有条件）**。核心条件满足：0 fatal、0 known illegal movement、
0 known reverse routing、核心网络 ≥500 deterministic OD 可达 94%（目标 ≥90%）。
边界断头与公司 access 缺口为数据范围限制（P1-07 Germany scale 解决），非图错误。

## 验证项与结果

| 验证项 | 结果 | 说明 |
|---|---|---|
| fatal（重复 uid/自环 movement） | **0** | 0 自环、0 重复 road uid |
| known illegal movement | **0** | 自环清零（P1-05 NavNode 图修正） |
| known reverse routing | **0** | 方向假设实验：单行道翻转后最大分量 OD 94%→2.4%——lanes_right-only→node0→node1 假设正确 |
| OD ≥500 deterministic | **核心网络 94.0%** | seed=20260810；全图口径 64%（见边界断头说明） |
| 十字路口 spot check | ✓ | mod_ger_47 等 movement 结构完整（mv=6/12） |
| company access | 13/15 | 2 个公司 prefab 节点无道路引用（P1-08 POI 细化） |
| fuel | 3/3 | NodeUid 全部接入路由网络 |
| service | 53/53 | 同上 |
| garage | 6/6 | 同上 |

## 关键事实与判定依据

### 方向语义验证（known reverse routing）
P1-05 方向假设：仅右车道（lanes_right）→ node0→node1；仅左车道 → 反向；双侧 → 双向。
验证实验：将全部单行道方向翻转后重建图——最大分量内有向 OD 从 **94.0% 降至 2.4%**。
结论：方向假设与游戏数据一致，无系统性逆向路由。

### 单行道死端 20 条（非错误）
死端判定：单向 road 的入口端（node0）无任何入边。抽样分析：node0 均无 prefab 引用、
node1 有 prefab 引用——即从 sector 边界进入的单向道路（跨 sector 引用），
8 sector 数据范围内起点在未加载 sector。P1-07 加载全德国 sector 后自然合并。

### OD 口径说明
- 核心网络（最大无向分量 2965 节点）内有向 OD：**94.0%**（单行道网络的真实水平）
- 全图有边节点口径：64%——采样包含 83 个边界断头小分量（<30 节点），
  跨分量 OD 不可达属数据范围限制，非图结构缺陷
- 翻转实验证明方向无系统性错误

### 分量结构
84 个无向分量：最大 2965（Berlin 核心网），其余 83 个均 <30 节点
（边界断头 road 团 + 装饰 prefab）。核心网内部结构完整。

## 已知限制（不阻断 Gate）

1. **转向类型判定为几何近似**（TurnAngle <30° 直行等）——P2 lane guidance 前需用
   曲线旋转/车道数据细化；Gate 不依赖 turn 精度
2. **公司 access 2/15 缺失**：linked prefab 节点无任何道路引用——P1-08 POI 处理
3. **movement 多车道合并**：NavNode 连接的多条平行曲线仅取第一条——P1-09
   车道级语义时展开
4. **环岛逐类验证**：Berlin 8 sector 未定位到典型环岛 prefab 实例（命名无 r* 特征）；
   环岛结构正确性依赖 movement 数据本身（PPD 官方数据，无人工合成边）——
   Germany scale 后补环岛实例验证

## 通过条件（正式）

Berlin Gate 通过，允许进入 P1-07 Germany Scale Test。
复核命令：

```text
map-inspector --install <游戏根> --sectors <柏林8> --gate
```
