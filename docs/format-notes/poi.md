# POI 格式笔记（P1-08）

来源：P1-08 POI / Search（2026-08）。PoiExtractor（ScsMapModel）+ search.db（SQLite FTS5）。

## POI 类型与数据源

| 类型 | 数据源 | access 机制 |
|---|---|---|
| City | CityItem.NodeUid | item 节点 |
| Company | CompanyItem + SemanticCompany | linked prefab 入口节点（roadTouched 第一个） |
| Garage | GarageItem.NodeUid | item 节点 |
| Fuel | FuelPumpItem + ServiceItem(GasStation) | item 节点 |
| Repair | ServiceItem(ServiceStation) | item 节点 |
| Rest | PPD SpawnPoint（TruckStop=5/Hotel=8） | prefab 节点 |
| Ferry/Train | FerryItem（IsTrain 区分） | item 节点 |
| Toll/Border | **无 item 数据源**（1.60 无 TollGate/Border item） | 明确标注非 routing（P2 经 prefab/sign 识别） |

## search.db 结构

```sql
CREATE TABLE poi (id INTEGER PRIMARY KEY, type TEXT, name TEXT, x REAL, z REAL,
                  access_node TEXT, meta TEXT);
CREATE VIRTUAL TABLE poi_fts USING fts5(name, type, content='poi', content_rowid='id');
CREATE TRIGGER poi_ai AFTER INSERT ON poi
  BEGIN INSERT INTO poi_fts(rowid, name, type) VALUES (new.id, new.name, new.type); END;
```

## 已知限制（P1 收官评审记录）

- CompanyItem 无 NodeUid——POI 位置 = access 节点（视觉位置待 P2）
- Rest POI 坐标为 PPD 局部坐标（未做 prefab 变换）——access 节点正确，显示位置待 P2
- Company access=0（无 roadTouched 节点）时 PoiExtractor 静默丢弃——"非 routing=0"
  是筛选后口径（Germany 5 家）
