// oracle-conn：跨实现数据级 oracle（ADR-005）。
// 目的：用 TruckLib（独立实现，GPL）解析同一批 sector，按统一文本格式转储
// road / prefab / ferry 三类 item 的 uid 与节点引用，供与本项目解析器逐项 diff——
// 判定本项目是否存在「解析丢数据」类缺陷（P5 断簇排查收尾）。
//
// 用法：oracle-conn --map <europe.mbd 路径> --sectors sec-0003-0001,sec-0003-0002,...
// 输出（行序按 uid 升序，便于 diff）：
//   R <roadUid:x16> <node0Uid:x16> <node1Uid:x16>
//   P <prefabUid:x16> <nodeUid:x16>...
//   F <ferryUid:x16> <nodeUid:x16> <port> <isTrain>
// GPL-3.0 — ETS2Nav 项目

using TruckLib.ScsMap;

var a = Environment.GetCommandLineArgs().Skip(1).ToArray();
string? mbd = Arg(a, "--map");
var secArg = (Arg(a, "--sectors") ?? "").Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
if (mbd == null || secArg.Length == 0)
{
    Console.Error.WriteLine("用法: oracle-conn --map <europe.mbd> --sectors sec-0003-0001,...");
    return 2;
}

var coords = new List<SectorCoordinate>();
foreach (var s in secArg)
{
    // sec-0003-0001 / sec+0002+0003 → X=-3, Z=-1 / X=2, Z=3
    var m = System.Text.RegularExpressions.Regex.Match(s, @"^sec([+-])(\d{4})([+-])(\d{4})$");
    if (!m.Success)
    {
        Console.Error.WriteLine($"无法解析 sector 名: {s}");
        return 2;
    }
    int x = int.Parse(m.Groups[2].Value) * (m.Groups[1].Value == "-" ? -1 : 1);
    int z = int.Parse(m.Groups[4].Value) * (m.Groups[3].Value == "-" ? -1 : 1);
    coords.Add(new SectorCoordinate(x, z));
}

Console.Error.WriteLine($"oracle: 加载 {coords.Count} 个 sector（TruckLib）…");
var map = Map.Open(mbd, coords);
Console.Error.WriteLine($"oracle: MapItems={map.MapItems.Count}");

var roads = new List<(ulong Uid, ulong N0, ulong N1)>();
var prefabs = new List<(ulong Uid, ulong[] Nodes)>();
var ferries = new List<(ulong Uid, ulong Node, string Port, bool Train)>();
foreach (var item in map.MapItems.Values)
{
    switch (item)
    {
        case Road r:
            roads.Add((r.Uid, r.Node.Uid, r.ForwardNode.Uid));
            break;
        case Prefab p:
            prefabs.Add((p.Uid, p.Nodes.Select(n => n.Uid).ToArray()));
            break;
        case Ferry f:
            ferries.Add((f.Uid, f.Node.Uid, f.Port.ToString(), f.TrainTransport));
            break;
    }
}
roads.Sort((x, y) => x.Uid.CompareTo(y.Uid));
prefabs.Sort((x, y) => x.Uid.CompareTo(y.Uid));
ferries.Sort((x, y) => x.Uid.CompareTo(y.Uid));

foreach (var (uid, n0, n1) in roads) Console.WriteLine($"R {uid:x16} {n0:x16} {n1:x16}");
foreach (var (uid, nodes) in prefabs) Console.WriteLine($"P {uid:x16} {string.Join(' ', nodes.Select(n => n.ToString("x16")))}");
foreach (var (uid, node, port, train) in ferries) Console.WriteLine($"F {uid:x16} {node:x16} {port} {train}");
Console.Error.WriteLine($"oracle: roads={roads.Count} prefabs={prefabs.Count} ferries={ferries.Count}");
return 0;

static string? Arg(string[] a, string key)
{
    for (int i = 0; i < a.Length - 1; i++) if (a[i] == key) return a[i + 1];
    return null;
}
