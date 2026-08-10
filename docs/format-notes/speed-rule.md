# Speed Rule 格式笔记（P1-09）

来源：P1-09 Road Rules / Signs / Speed（2026-08，游戏 1.60.1.7）。

## 限速数据链

```
road item TrafficRule 字段（仅 ~4% 设置——大部分为空）
  ↓（空时）
road look 的 lanes_left/right（traffic_lane.road.local 等）
  ↓
traffic_lane 定义（/def/world/traffic_lane.sii）的 speed_class（local_road/expressway/motorway/rail_*）
  ↓
country 限速表（/def/country/<name>/speed_limits.sii）：
  country_speed_limit → vehicle_speed_class × lane_speed_class[] 并行数组
  → limit[]（乡村）/ urban_limit[]（城市）
```

## 三态语义（P1 收官评审 M4 修复）

| SpeedLimit 值 | 语义 |
|---|---|
| -1 | **未知**（国家/限速表/speed_class 缺失——不得当作无限速） |
| 0 | **无限速**（仅当国家表 truck 行确实为 0——实测德国 truck motorway=80，car motorway=0；0 属极少数国家/类别） |
| >0 | 数值 km/h |

## 国家判定（P1 收官评审 M5 修复）

- 城市 bbox 判定：CityItem 的 Width/Height 矩形（半宽高下限 800m）
- 禁止半径圆（边境城市会误判——柏林东北角曾被 szczecin 半径 7200m 覆盖，
  6.3% road 被套波兰限速表）
- 无城市覆盖 → 默认 germany（测试范围口径；Europe 需国家边界数据——P2）

## 已知缺口

- speed segments（同道路限速分段——城市边界处 50↔60）未实现（P2）
- 动态限速 sign（德国高速可变牌）未识别（P2 视觉/文本）
- telemetry 实测一致率未采集（P1-09 完成条件裁剪延后 P2——
  工具 speed-validator 就绪，共享内存布局经核对正确）
