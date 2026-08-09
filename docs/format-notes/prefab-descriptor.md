# Prefab Descriptor（PPD）格式笔记（1.60）

来源：P1-04 Prefab Navigation Parser（2026-08 实测，游戏版本 1.60.1.7）。
对照 oracle：TruckLib.Models `Ppd/` 命名空间源码（vendor/ref/TruckLib.Models，MIT）。

## 文件位置与映射链

```
sector PrefabItem.Model（无前缀，如 mod_ger_67）
  → /def/world/prefab*.sii 的 prefab_model unit（名 prefab.mod_ger_67）
  → prefab_desc 属性（"/prefab2/cross_temp/ger/xxx.ppd"）
  → PPD 二进制解析
```

- 1.60 主 prefab 描述在 `/prefab2/`（8467 个 .pmd 模型；prefab_desc 指向同名 .ppd）
- prefab.sii 中旧条目指向 `/prefab/`（597 个旧式）
- **与 road type 相同**：sector 引用无 `prefab.` 前缀，需先补前缀查 prefab_model unit

## PPD 二进制结构（v0x19，1.60 实测）

```
u32 version（0x19）
11×u32 count：nodes / navCurves / signs / semaphores / spawnPoints /
              terrainPoints / terrainPointVariants / mapPoints / triggerPoints /
              intersections / navNodes
12×u32 offsets（可忽略——顺序布局）
按序段：
  ControlNode × n：4×u32(terrain idx) + pos(12) + dir(12) + 8×i32 in + 8×i32 out   [104 B]
  NavCurve × n：见下                                                              [132 B]
  Sign × n：token+vec3+quat+token+token                                            [52 B]
  Semaphore × n：见下                                                              [84 B]
  SpawnPoint × n：pos+quat+u32+u32                                                [36 B]
  TerrainPointPos × n、TerrainPointNorm × n：vec3
  TerrainPointVariant × n：2×u32
  MapPoint × n：visFlags u32 + navFlags u32 + pos + 6×i32 + used u32               [48 B]
  TriggerPoint × n：u32+token+3×f32+u32+vec3+2×i32                                 [48 B]
  Intersection × n：curveId u32 + pos f32 + radius f32 + flags u32                 [16 B]
  NavNode × n：见下                                                                [188 B]
```

## NavCurve（导航曲线——P1-04 核心，132 B）

```
token Name → u32 flags → 4×u8 (EndNode,EndLane,StartNode,StartLane)
→ StartPos vec3 → EndPos vec3 → StartQuat → EndQuat → Length f32
→ 4×i32 NextLines → 4×i32 PreviousLines → u32 nextUsed → u32 prevUsed
→ i32 SemaphoreId → token TrafficRule → u32 NavNodeIndex
```

**flags 位语义**（TruckLib.Models NavCurve）：
- bit2-3 Blinker（0=none 1=left 2=right 3=both）
- bit5-6 AllowedVehicles（0=car 1=truck 2=bus 3=all）
- bit13 LowProbability、bit14 LimitDisplacement、bit15 AdditivePriority
- bit16-19 PriorityModifier（nibble）

**导航语义**：
- `IsEntry = PreviousCount == 0`（无前驱 = prefab 边界进入）
- `IsExit = NextCount == 0`（通向 prefab 边界）
- NextLines 索引 = 有向连接（合法行车方向）
- LeadsToNodes（StartNode/StartLane/EndNode/EndLane）= 曲线端点绑定的 prefab node
  ——prefab node 索引对应 sector PrefabItem.Nodes 数组
- SemaphoreId → Semaphores[]（-1 = 无灯）

## Movement 恢复（P1 计划 §21/22）

prefab 连通性完全来自 navigation 语义：从每个 entry curve 沿 NextLines 链
DFS 到 exit curve，每条完整路径 = 一个 movement：

```text
movement = { entry_curve, exit_curve, entry_node, exit_node,
             curve_path[], length, semaphore_id, priority, turn_type }
```

- 转向类型（角度近似）：|Δ|<30° 直行 / >150° U 型 / 左负右正
- 信号灯：路径上从出口向前找第一个带灯的曲线
- **禁止** prefab 全连接（foreach connector A,B AddEdge）

## Token 注意

- PPD 内 token 为 8 字节 base-38（`\0 0-9 a-z _`）编码，**最大 12 字符**
- 含 `.`/`/` 或超长名 → **哈希 token**（值 ≥ 38^12），数学解码产生假名——
  解析器返回 `&0x…` 标记不崩溃；Name/Profile 不参与导航语义（SemaphoreData
  同时暴露 TokenRaw 原始值）
- 字符集实现必须用字符数组（C# `"\000…"` 字符串转义有歧义——实测多出字符）

## 实现

- `ScsPrefab/PpdReader.cs`：v0x19 解析（非导航段跳过字节）
- `ScsPrefab/PrefabResolver.cs`：token → prefab.sii 映射 → PPD 加载（缓存 + FailedPpds 诊断）
- `ScsPrefab/PrefabMovements.cs`：movement 恢复
- 验证：`map-inspector --install <游戏根> --sectors <柏林> --prefab [token]`
  ——Berlin 220/220 prefab 加载，928 movements，277 信号灯绑定
