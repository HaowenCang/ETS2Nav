# Road Look 格式笔记（1.60）

来源：P1-03 Definition Layer（2026-08 实测，游戏版本 1.60.1.7）。

## 文件位置

| 文件 | 内容 |
|---|---|
| `/def/world/road_look.sii` | 传统（legacy）road look 定义（unit 名 `road.look0` 等） |
| `/def/world/road_look.template.sii` | 模板化 look（1.5x+）：unit 名 `road.template0`、`road.at1` 等 |
| `/def/world/road_look.template.mod_*.sii` | mod 覆盖/扩展（如 universal_roads） |
| `/def/world/traffic_lane.sii` | traffic lane 定义（`traffic_lane.road.local` 等） |
| `/def/country/<country>/speed_limits.sii` | 国家限速 |

## Sector 引用语义（关键）

sector 的 road item 中 **RoadType token 为无前缀名**（如 `at1`、`blke33`），
definition unit 名为 `road.at1`。解析时必须先尝试 `road.` + 名，再回退裸名
（与 ETS2LA `GetRoadUnit("road." + roadType)` 逻辑一致）。

Berlin 8 sector 实测：50 种 road type 引用全部解析（0 缺失）。

## Road item 二进制布局（RoadType 及周边）

对照 TruckLib `RoadSerializer.cs`（1.60）：

```
kdop(53B) → road_flags(u32) → RoadType(token)
→ RightTrafficRule → LeftTrafficRule → RightVariant → LeftVariant
→ RightEdgeRight/Left → LeftEdgeRight/Left（4 token）
→ terrain profile×2（token+float）
→ RightLook/LeftLook → material
→ railing×3（每侧 token+int16）→ height offset×2（int32）
→ Node0(u64) → Node1(u64) → Length(float)
```

**历史教训**：P0 曾把 TrafficRule/Variant 字段误读为 lanes/template 字段，
因只对照了 item 数量与 UID 而未发现。P1-03 对照 TruckLib 源码修正。

## Road look definition 结构

```
road_look : road.at1
{
    name: "at road 1 one way"
    template_right: "/road_template/at/at_road_1_one_way.pmd"   # 视觉模板（pmd 模型）
    lanes_right[]: traffic_lane.road.local                      # 车道 → lane 定义
    compatible_edges_right[]: ger_sw_3m_a                       # 兼容边缘（视觉）
    road_offset / road_size_left / road_size_right
    shoulder_size_left / shoulder_size_right          # 属性名为 shoulder_size_*（实测；早期笔记误记 space）
    center_line_left_style: 3                                    # 线型编号
    template_variants_right[]: .tmpl_var.road.template1          # 变体引用（tmpl_var 块）
}
```

## Traffic lane 定义结构

```
traffic_lane_data : traffic_lane.road.local
{
    speed_class: local_road          # local_road/expressway/motorway
    rank: 50
    traffic_rules[]: traffic_rule.road
    traffic_rules[]: traffic_rule.overtake_alw   # 允许对向车道超车
}
```

**方向语义**：`overtake_alw`（traffic_lane.road.local.overtake 等）允许在对向
车道超车 → 1+1 单车道路段可双向通行；无此规则的 lane 只在同向车道行驶。
**lane 决定道路方向语义**（R2 风险的关键输入）。

## 国家限速（并行数组）

```
country_speed_limit : .speed_limit.car {
    vehicle_speed_class: car
    lane_speed_class[]: local_road
    limit[]: 100          # 与 lane_speed_class[] 索引对齐
    urban_limit[]: 50
    max_limit[]: 60       # 可缺省（并行数组按索引对齐，缺省 0）
}
```

unit 名为 local（`.speed_limit.car`），**无国家信息**——必须按文件路径
`/def/country/<name>/speed_limits.sii` 归类国家。

## SII 语法要点（corpus 实测）

- `.sui` include 片段**无 SiiNunit 包装**（裸 unit 序列）
- `@include "city/berlin.sui"` 相对路径、可嵌套、需防循环
- `unit : name {` 块与 header 可同行（country_speed_limit 风格）
- `//` 行注释与 `#` 并存（官方文件实测）
- `name[]: v` 并行数组：同 key 多次出现按序累积

## 实现

- `ScsDefinitions/DefinitionLoader.cs`：@include 递归展开
- `ScsDefinitions/Models.cs`：RoadLookDefinition / TrafficLaneDefinition /
  CountryDefinition / CityDefinition / CompanyDefinition / FerryDefinition
- `ScsDefinitions/DefinitionResolver.cs`：预加载 + token 查询
- 验证：`map-inspector --install <游戏根> --sectors <柏林> --defs`
