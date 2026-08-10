using ScsPrefab;
using ScsSector;

namespace ScsMapModel;

/// <summary>POI 类型（P1 计划 §101）。</summary>
public enum PoiType
{
    City, Company, Garage, Fuel, Repair, Rest, Ferry, Train, Toll, Border,
}

/// <summary>POI 条目：名称、类型、位置、routing access（null = 非 routing POI）。</summary>
public sealed class PoiEntry
{
    public required PoiType Type { get; init; }
    public required string Name { get; init; }
    public required double X { get; init; }
    public required double Z { get; init; }
    /// <summary>routing access 节点 uid；null = 非 routing POI（无道路接入，如 toll/border 无数据源）。</summary>
    public ulong? AccessNodeUid { get; init; }
    public string? Meta { get; init; }
}

/// <summary>
/// POI 提取器（P1 计划 §101）：city/company/garage/fuel/repair/rest/ferry/train/toll/border。
/// access 语义：item 自带 NodeUid（city/company/garage/fuel/service/ferry）→ 图接入；
/// rest 来自 PPD SpawnPoint（TruckStop/Hotel）；toll/border 无 item 数据源 → 非 routing POI。
/// </summary>
public static class PoiExtractor
{
    public static List<PoiEntry> Extract(IEnumerable<SectorFile> sectors, SemanticMap map,
        PrefabResolver prefabs, IDictionary<string, ScsDefinitions.FerryDefinition>? ferryDefs = null)
    {
        var result = new List<PoiEntry>();
        var nodePos = sectors.SelectMany(s => s.Nodes).GroupBy(n => n.Uid)
            .ToDictionary(g => g.Key, g => (g.First().X, g.First().Z));
        var companyNames = map.Companies.ToDictionary(c => c.Uid, c => c.CompanyName);

        foreach (var sec in sectors)
        {
            foreach (var c in sec.Items.OfType<CityItem>())
                Add(result, PoiType.City, c.City, c.NodeUid, nodePos, null);
            foreach (var c in sec.Items.OfType<CompanyItem>())
            {
                var name = companyNames.TryGetValue(c.Uid, out var n) ? n : c.CompanyName;
                var access = map.Companies.FirstOrDefault(x => x.Uid == c.Uid)?.AccessNodeUid;
                // CompanyItem 无 NodeUid——位置取 access 节点（linked prefab 的入口）
                if (access is ulong a && a != 0)
                    Add(result, PoiType.Company, name, a, nodePos, access, meta: $"prefab={c.LinkedPrefabUid:x8}");
            }
            foreach (var g in sec.Items.OfType<GarageItem>())
                Add(result, PoiType.Garage, "garage", g.NodeUid, nodePos, null);
            foreach (var f in sec.Items.OfType<FuelPumpItem>())
                Add(result, PoiType.Fuel, "fuel", f.NodeUid, nodePos, null);
            foreach (var sv in sec.Items.OfType<ServiceItem>())
            {
                // ServiceType：0=GasStation 1=ServiceStation 2=TruckDealer 4=Parking 5=Recruitment 7/8=WeighStation
                var t = sv.SpawnPointType switch
                {
                    0 => PoiType.Fuel,
                    1 => PoiType.Repair,
                    2 => PoiType.Company,   // 卡车经销商（近似 company）
                    4 => PoiType.Rest,       // parking（近似 rest）
                    _ => (PoiType?)null,
                };
                if (t != null) Add(result, t.Value, t.Value.ToString(), sv.NodeUid, nodePos, null);
            }
            foreach (var f in sec.Items.OfType<FerryItem>())
            {
                var type = f.IsTrain ? PoiType.Train : PoiType.Ferry;
                Add(result, type, type.ToString(), f.NodeUid, nodePos, null);
            }
        }

        // rest：prefab 的 PPD SpawnPoint（TruckStop=5/Hotel=8）——映射到 prefab 节点（routing access）
        var prefabNodes = map.Junctions.ToDictionary(j => j.Uid, j => j.NodeUids);
        foreach (var j in map.Junctions)
        {
            var pd = prefabs.Load(j.PrefabToken);
            if (pd == null) continue;
            foreach (var sp in pd.SpawnPoints)
            {
                if (sp.Type is not (5 or 8)) continue;   // TruckStop/Hotel
                var access = j.NodeUids.FirstOrDefault();
                var name = sp.Type == 8 ? "hotel" : "truck_stop";
                result.Add(new PoiEntry
                {
                    Type = PoiType.Rest,
                    Name = name,
                    X = sp.X, Z = sp.Z,
                    AccessNodeUid = access != 0 ? access : null,
                    Meta = $"prefab={j.PrefabToken}",
                });
            }
        }

        // toll/border：无 item 数据源——明确标为非 routing POI（占位说明）
        // （SCS 1.60 无 TollGate/Border item；收费站/边境为 prefab+sign，识别留给 P2）
        return result;
    }

    private static void Add(List<PoiEntry> list, PoiType type, string name, ulong nodeUid,
        Dictionary<ulong, (double X, double Z)> nodePos, ulong? access, string? meta = null)
    {
        if (nodeUid == 0) return;
        var pos = nodePos.TryGetValue(nodeUid, out var p) ? p : (0, 0);
        list.Add(new PoiEntry
        {
            Type = type,
            Name = name,
            X = pos.X, Z = pos.Z,
            AccessNodeUid = access ?? nodeUid,   // 默认用 item 节点本身（有 node 即接入）
            Meta = meta,
        });
    }
}
