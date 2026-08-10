# P1-08 POI / Search 验证报告（2026-08）

依据：P1-map-compiler-plan.md §101。数据：游戏 1.60.1.7。
工具：`map-inspector --install <根> --sectors <柏林>|--region germany --searchdb <out.db>`。

## 结论

**通过**。Berlin 79 POI / Germany 375 POI 全部拥有有效 routing access
（非 routing = 0）。search.db（SQLite FTS5，ADR-004）生成并验证 FTS 查询。

## POI 覆盖

| 类型 | Berlin 8 | Germany 39 | 数据源 | access 机制 |
|---|---|---|---|---|
| City | 7 | 41 | CityItem.NodeUid | item 节点 |
| Company | 16 | 84 | CompanyItem + SemanticCompany | linked prefab 入口节点 |
| Garage | 6 | 26 | GarageItem.NodeUid | item 节点 |
| Fuel | 33 | 164 | FuelPumpItem + ServiceItem(GasStation) | item 节点 |
| Repair | 3 | 14 | ServiceItem(ServiceStation) | item 节点 |
| Rest | 14 | 41 | PPD SpawnPoint(TruckStop=5/Hotel=8) | prefab 节点 |
| Ferry | 0 | 5 | FerryItem.NodeUid | item 节点 |
| Train | 0 | 0 | FerryItem(IsTrain) | item 节点 |
| Toll / Border | — | — | **无 item 数据源**（1.60 无 TollGate/Border item；收费站/边境为 prefab+sign） | 明确标注：非 routing（P2 识别） |

非 routing POI：**0**（Berlin 与 Germany 均全部有 access）。

## search.db 格式（ADR-004）

SQLite 数据库：
- `poi` 表：id/type/name/x/z/access_node(hex uid)/meta
- `poi_fts`：FTS5 虚拟表（name/type 全文索引，content 外置表 + 触发器同步）
- 验证：`SELECT ... FROM poi_fts WHERE poi_fts MATCH 'berlin'` 命中 4 条 City

## 已知限制（不阻断）

1. **toll/border 无 item 数据源**：1.60 的收费站/边境是 prefab+sign 组合，
   无独立 item 类型——P1-08 明确标注为"非 routing POI（P2 经 prefab/sign 识别）"
2. **rest 位置为 PPD 局部坐标**：SpawnPoint 的 X/Z 是 prefab 局部坐标（未做
   prefab 变换）——access 节点（地图坐标）正确，显示位置待 P2 修正
3. **公司 POI 位置 = access 节点**（CompanyItem 无 NodeUid——视觉位置待 P2）
4. **Repair 近似**：ServiceType=1（ServiceStation）作为 repair；SCS 的 repair
   实际分布在 company（如 "Repair" 品牌公司）——P2 细化

## 通过条件（正式）

P1-08 通过。下一步 P1-09 Road Rules / Signs / Speed。
