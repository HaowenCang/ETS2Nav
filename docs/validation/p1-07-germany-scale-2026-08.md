# P1-07 Germany Scale Test 验证报告（2026-08）

依据：P1-map-compiler-plan.md §100。数据：游戏 1.60.1.7。
范围：德国区域 39 sector（x∈[-1,3]、z∈[-6,3] 的 4096 网格——按德国城市坐标 bbox 实测），
复核命令 `map-inspector --install <根> --region germany --semantic/--gate`。

## 结论

**通过**。完整 Germany build 成功；validation 无 blocker（0 fatal、自环 0）；
≥500 deterministic OD：核心网络 **94.8%**（目标 ≥90%）；fuel/service/garage 全通过。

## Germany build 规模

| 指标 | Berlin 8 | Germany 39 | Europe 679（勘误前） |
|---|---|---|---|
| sector | 8 | 39 | 679 |
| roads | 2,429 | 11,551 | 143,677（v2 rail 排除后；v1 149,814） |
| junctions | 693 | 3,323 | 42,843 |
| companies | 15 | 80 | 1,185 |
| road look 种类 | 50 | **82** | — |
| prefab 种类 | 220 | **485** | — |
| 单行道 | 1,157 | 5,029 | — |
| RoutingGraph edges | 6,207（rail 排除修复后；原记 10,848 为 P1-05 修复前） | 29,585 | 426,907（v2；v1 435,392） |
| 最大连通分量 | 2,965 | 13,436 | 165,790 |
| **核心网 OD 500** | **95.4%**（收官修复后口径；原记 94.0%） | **94.6%**（收官修复后；原记 94.8%） | **99.4%** |

## Scale 质量结论

1. **规模扩展稳定**：OD 从 Berlin 94.0% → Germany 94.8% → Europe 99.4%——
   图质量随规模提升（边界断头占比下降），无 scale 退化
2. **多样性暴露**：prefab 485 种（Berlin 220）、road look 82 种（Berlin 50）——
   全部经同一解析链路（PPD v0x19 + road look 方向语义）——无新解析失败
3. **构建性能**：全欧洲 679 sector 语义图构建 **2.3s**（加载 1.1s + build 1.0s +
   图 0.25s）——编译器规模可行
4. **全欧洲最大分量 165,790 节点、435,392 边**——为 P1-13 全量编译奠定基础

## Gate 复核（Germany）

| 检查项 | 结果 |
|---|---|
| fatal | **0** |
| 自环 movement | 0 |
| 单行死端 | 117（均为区域边界断头——德国范围边界外道路） |
| 全图 OD 口径 | 61.8%（含边界断头小分量） |
| 核心网 OD 口径 | **94.8%** |
| 公司 access | 75/80（5 个 prefab 节点无道路引用） |
| fuel / service / garage | 13/13 / 224/224 / 26/26 |

## 已知事项（不阻断）

1. 德国范围按城市 bbox 近似（含少量邻国城市如 szczecin）——scale test 目的达成即可
2. 公司 5/80 access 缺失：linked prefab 节点无道路引用——P1-08 POI 处理
3. 单行死端全部为范围边界断头（RoadGraph 层面无错误）

## 通过条件（正式）

Germany Scale Test 通过。下一步 P1-08 POI / Search。
