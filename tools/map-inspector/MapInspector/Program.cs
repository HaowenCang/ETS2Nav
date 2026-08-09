// map-inspector（P1 §44）：按 UID/坐标/sector/road/prefab 查询地图数据 + 运行验证。
// 用法：
//   map-inspector --dir <sector目录> --sectors <sec+0002+0003,...> [查询]
//   查询：--uid <hex> | --coord x,z | --sector <name> | --edges <node-uid> |
//         --validate | --stats | --geojson <out.json>
// GPL-3.0 — ETS2Nav 项目

using System.Text.Json;
using ScsGraph;
using ScsSector;
using ScsValidation;
using ScsValidation.Validators;

var cmdArgs = Environment.GetCommandLineArgs().Skip(1).ToArray();
string dir = Arg(args, "--dir") ?? @"E:\Projects\Pi\ETS2Nav\vendor\extracted\base_map\map\europe";
var secNames = (Arg(args, "--sectors") ?? "sec+0002-0002,sec+0002-0003,sec+0003-0002,sec+0003-0003,sec+0002-0001,sec+0002-0004,sec+0003-0001,sec+0003-0004")
    .Split(',', StringSplitOptions.RemoveEmptyEntries);

var sectors = new List<SectorFile>();
foreach (var name in secNames)
{
    var path = Path.Combine(dir, name + ".base");
    if (!File.Exists(path)) { Console.Error.WriteLine($"缺 sector: {name}"); continue; }
    sectors.Add(SectorFile.Read(path));
}
Console.WriteLine($"已加载 {sectors.Count} 个 sector（{sectors.Sum(s => s.Items.Count)} items / {sectors.Sum(s => s.Nodes.Count)} nodes）");
var graph = RoadGraph.Build(sectors);

string? Arg(string[] a, string key)
{
    for (int i = 0; i < a.Length - 1; i++)
        if (a[i] == key) return a[i + 1];
    return null;
}

if (cmdArgs.Contains("--stats"))
{
    Console.WriteLine($"节点 {graph.NodeCount}，边 {graph.EdgeCount}");
    foreach (var g in sectors.GroupBy(s => s.SectorName).OrderBy(g => g.Key))
        Console.WriteLine($"  {g.Key}: {g.Sum(s => s.Items.Count)} items / {g.Sum(s => s.Nodes.Count)} nodes");
}

if (cmdArgs.Contains("--validate"))
{
    var report = new ValidationEngine()
        .Register(new StructuralValidator())
        .Register(new ReferenceValidator())
        .Register(new DirectionValidator())
        .Register(new ConnectivityValidator())
        .Register(new GeometryValidator())
        .Run(new ValidationContext { Sectors = sectors, Graph = graph });
    Console.WriteLine($"验证: {report}");
    foreach (var i in report.Issues.Where(i => i.Severity != ValidationSeverity.Info).Take(50))
        Console.WriteLine($"  {i}");
}

var uidArg = Arg(args, "--uid");
if (uidArg != null)
{
    var uid = Convert.ToUInt64(uidArg, 16);
    var found = sectors.SelectMany(s => s.Items).FirstOrDefault(i => i.Uid == uid);
    if (found == null) Console.WriteLine($"UID {uid:x16} 未找到");
    else Console.WriteLine($"UID {uid:x16}: {found.GetType().Name} {JsonSerializer.Serialize(found, new JsonSerializerOptions { WriteIndented = true })}");
}

var edgesArg = Arg(args, "--edges");
if (edgesArg != null)
{
    var uid = Convert.ToUInt64(edgesArg, 16);
    if (!graph.TryGetNodeIndex(uid, out int idx))
    {
        Console.WriteLine($"节点 {uid:x16} 不在图中");
    }
    else
    {
        var (x, y, z) = graph.Positions[idx];
        Console.WriteLine($"节点 {uid:x16} (idx {idx}) pos=({x:F1},{y:F1},{z:F1})");
        foreach (int e in graph.OutEdges(idx))
        {
            var (from, to) = graph.EdgeEnds(e);
            var data = graph.Edge(e);
            var (fx, fy, fz) = graph.Positions[from];
            var (tx, ty, tz) = graph.Positions[to];
            Console.WriteLine($"  边[{e}] {fx:F1},{fz:F1} -> {tx:F1},{tz:F1}  uid={data.ItemUid:x16}  look={data.RoadLook}  len={data.Length:F0}m  prefabConn={data.IsPrefabConnector}");
        }
    }
}

var coordArg = Arg(args, "--coord");
if (coordArg != null)
{
    var parts = coordArg.Split(',');
    double cx = double.Parse(parts[0]), cz = double.Parse(parts[1]);
    double best = double.MaxValue;
    int bestNode = -1;
    for (int n = 0; n < graph.NodeCount; n++)
    {
        var (x, _, z) = graph.Positions[n];
        double d = (x - cx) * (x - cx) + (z - cz) * (z - cz);
        if (d < best) { best = d; bestNode = n; }
    }
    if (bestNode >= 0)
    {
        Console.WriteLine($"最近节点 idx {bestNode} 距离 {Math.Sqrt(best):F1}m");
        foreach (int e in graph.OutEdges(bestNode))
        {
            var (from, to) = graph.EdgeEnds(e);
            var data = graph.Edge(e);
            var (fx, _, fz) = graph.Positions[from];
            var (tx, _, tz) = graph.Positions[to];
            Console.WriteLine($"  边[{e}] {fx:F1},{fz:F1} -> {tx:F1},{tz:F1}  uid={data.ItemUid:x16}  {data.RoadLook}");
        }
    }
}

var sectorArg = Arg(args, "--sector");
if (sectorArg != null)
{
    var sec = sectors.FirstOrDefault(s => s.SectorName == sectorArg);
    if (sec == null) Console.WriteLine($"sector {sectorArg} 未加载");
    else
    {
        Console.WriteLine($"{sec.SectorName}: {sec.Items.Count} items / {sec.Nodes.Count} nodes / {sec.VisibilityAreaUids.Count} vis");
        foreach (var g in sec.Items.GroupBy(i => i.GetType().Name).OrderByDescending(g => g.Count()))
            Console.WriteLine($"  {g.Key}: {g.Count()}");
    }
}

var geojsonArg = Arg(args, "--geojson");
if (geojsonArg != null)
{
    var features = new List<object>();
    // 边 → LineString（含方向信息在属性）
    for (int e = 0; e < graph.EdgeCount; e++)
    {
        var (from, to) = graph.EdgeEnds(e);
        var data = graph.Edge(e);
        var (x0, _, z0) = graph.Positions[from];
        var (x1, _, z1) = graph.Positions[to];
        features.Add(new Dictionary<string, object>
        {
            ["type"] = "Feature",
            ["properties"] = new Dictionary<string, object>
            {
                ["kind"] = data.IsPrefabConnector ? "prefab" : "road",
                ["uid"] = data.ItemUid.ToString("x16"),
                ["from"] = graph.NodeUids[from].ToString("x16"),
                ["to"] = graph.NodeUids[to].ToString("x16"),
                ["look"] = data.RoadLook,
                ["len"] = data.Length,
            },
            ["geometry"] = new Dictionary<string, object>
            {
                ["type"] = "LineString",
                ["coordinates"] = new[] { new[] { x0, z0 }, new[] { x1, z1 } },
            },
        });
    }
    // 验证错误 → Point marker
    var report = new ValidationEngine()
        .Register(new StructuralValidator())
        .Register(new ReferenceValidator())
        .Register(new DirectionValidator())
        .Register(new GeometryValidator())
        .Run(new ValidationContext { Sectors = sectors, Graph = graph });
    foreach (var i in report.Issues.Where(i => i.Severity == ValidationSeverity.Error))
    {
        if (i.SourceUid == 0) continue;
        var item = sectors.SelectMany(s => s.Items).FirstOrDefault(it => it.Uid == i.SourceUid);
        if (item == null) continue;
        // 坐标取 item 的首个 node（图中查询）
        double mx = 0, mz = 0;
        var nodeUid = item is RoadItem r ? r.Node0 : item is PrefabItem pf && pf.NodeUids.Length > 0 ? pf.NodeUids[0] : 0;
        if (nodeUid != 0 && graph.TryGetNodeIndex(nodeUid, out int nidx))
        {
            var (nx, _, nz) = graph.Positions[nidx];
            mx = nx; mz = nz;
        }
        features.Add(new Dictionary<string, object>
        {
            ["type"] = "Feature",
            ["properties"] = new Dictionary<string, object>
            {
                ["kind"] = "validation_error",
                ["code"] = i.Code,
                ["desc"] = i.Description,
            },
            ["geometry"] = new Dictionary<string, object>
            {
                ["type"] = "Point",
                ["coordinates"] = new[] { mx, mz },
            },
        });
    }
    var fc = new Dictionary<string, object>
    {
        ["type"] = "FeatureCollection",
        ["features"] = features,
    };
    File.WriteAllText(geojsonArg, JsonSerializer.Serialize(fc));
    Console.WriteLine($"已导出 {features.Count} 个要素到 {geojsonArg}");
}
