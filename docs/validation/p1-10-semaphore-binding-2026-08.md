# P1-10 Semaphore Binding 验证报告（2026-08）

依据：P1-map-compiler-plan.md §103。数据：游戏 1.60.1.7。

## 结论

**通过**。JunctionMovement ↔ signal group（PPD SemaphoreId）绑定完成：
Berlin 157 灯路口 / 1,314 带灯 movement、Germany 663 灯路口 / 5,148 带灯
movement——**signal group 100% 确定**（P0 已测试路口 mod_ger_67 类全部覆盖）。

## 绑定模型（1.60 实测定案）

### 数据源发现
- **prefab item 的 SemaphoreProfile 字段在 1.60 几乎废弃**：Berlin 693 个 prefab 仅
  1 个设置（`cr_2x1_low`）——**灯的配置完全在 PPD 内部**
- PPD Semaphore.Profile（如 `1x1_rc`）是**编辑器内部灯组名**——不在任何 def .sii 中
  （全量 /def 扫描无命中）——不可映射到 semaphore_profile.sii

### Signal Group 语义
```text
signal group = PPD SemaphoreId（同 id 的灯同组——mod_ger_67 中 id=17 有 4 个灯）
JunctionMovement.SemaphoreId（PPD NavCurve 绑定）→ signal group
组类型 = PPD 灯 Type（SemaphoreType 枚举）：
  UseProfile=0（默认——类型在 PPD 内不可见 → null）
  TrafficLightMinor=3 / TrafficLightMajor=4 / TrafficLight=2（显式类型）
```

### 绑定结果
| 范围 | 灯路口 | 带灯 movement | signal group 确定 | 显式类型 |
|---|---|---|---|---|
| Berlin 8 | 157 | 1,314 | **1,314（100%）** | 0（全 UseProfile） |
| Germany 39 | 663 | 5,148 | **5,148（100%）** | 55 |

## 完成条件对照

> P0 已测试信号灯路口均能确定规划 movement 所受的 signal group

- movement.SemaphoreId（signal group）在 PPD 解析阶段绑定（NavCurve.SemaphoreId）✓
- P0 的 TL-01/02 测试路口（mod_ger_67 信号路口）movement 的 group 确定 ✓
- **group 类型（major/minor）在 UseProfile 灯上不可见**——P2 需从灯几何/
  方向推断（记录为已知限制）

## 已知限制

1. UseProfile 灯的组类型未知（major/minor 判定待 P2 几何推断）
2. PPD SemaphoreId ↔ ETS2LA 48B 灯数组索引的桥接未做（P2 灯匹配）
3. SemanticMapBuilder 每 junction 重复 Load PPD（有缓存，无性能问题）

## 通过条件（正式）

P1-10 通过。下一步 P1-11 Dataset Writer（manifest.json/map.db/routing.graph/junction.graph）。
