// map-inspector（P1 §44）：按 UID/坐标/sector/road/prefab 查询地图数据 + 运行验证。
// 用法：
//   map-inspector --dir <sector目录> --sectors <sec+0002+0003,...> [查询]
//   查询：--uid <hex> | --coord x,z | --sector <name> | --edges <node-uid> |
//         --validate | --stats | --geojson <out.json>
// GPL-3.0 — ETS2Nav 项目

using System.Text.Json;
using ScsGraph;
using ScsSector;
using ScsResource;
using ScsValidation;
using ScsValidation.Validators;

var cmdArgs = Environment.GetCommandLineArgs().Skip(1).ToArray();
var installDir = Arg(args, "--install");   // 游戏安装根目录（经 GameInstall+Overlay 直接读 .scs）
string dir = Arg(args, "--dir") ?? @"E:\Projects\Pi\ETS2Nav\vendor\extracted\base_map\map\europe";

// 资源层（P1-02）：所有 sector 读取经 IScsResourceProvider；上层不直接触碰文件系统
// DirectoryProvider/OverlayProvider 均实现 IDisposable（P1-03 评审 m2：句柄所有权）
using OverlayProvider overlay = installDir != null
    ? BuildInstallOverlay(installDir)
    : new OverlayProvider(new DirectoryProvider(dir));

var secNames = (Arg(args, "--sectors") ?? "")
    .Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
if (cmdArgs.Contains("--all-sectors"))
{
    secNames = overlay.Enumerate("/map/europe")
        .Where(p => p.EndsWith(".base") && p.Contains("/sec+"))
        .Select(p => Path.GetFileNameWithoutExtension(p))
        .OrderBy(n => n)
        .ToArray();
}
else if (secNames.Length == 0)
{
    secNames = new[] { "sec+0002-0002", "sec+0002-0003", "sec+0003-0002", "sec+0003-0003", "sec+0002-0001", "sec+0002-0004", "sec+0003-0001", "sec+0003-0004" };
}

var sectors = new List<SectorFile>();
foreach (var name in secNames)
{
    foreach (var ext in new[] { ".base", ".aux" })
    {
        var vp = $"/map/europe/{name}{ext}";
        if (!overlay.Exists(vp)) continue;
        using var s = overlay.Open(vp);
        sectors.Add(SectorFile.Read(s, name));   // SectorName 不带扩展名（P1-03 评审 m3）
    }
}
Console.WriteLine($"已加载 {sectors.Count} 个 sector（{sectors.Sum(s => s.Items.Count)} items / {sectors.Sum(s => s.Nodes.Count)} nodes）");
var graph = RoadGraph.Build(sectors);

// --install 模式：GameInstall.Detect + BuildOverlay（含 DLC 地图过滤）；返回 OverlayProvider（IDisposable）
static OverlayProvider BuildInstallOverlay(string root)
{
    var install = GameInstall.Detect(root);
    Console.WriteLine($"检测到游戏安装：v{install.GameVersion}，{install.Archives.Count} archives / {install.EnabledDlc.Count} DLC");
    return install.BuildOverlay();
}

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

if (cmdArgs.Contains("--prefab"))
{
    // P1-04 验证：prefab token → PPD 加载 + movement 恢复
    var res = new ScsPrefab.PrefabResolver(overlay);
    var token = Arg(args, "--prefab") ?? "";
    if (token.Length == 0)
    {
        // 无参数：Berlin sector 全部 prefab 覆盖验证
        var tokens = sectors.SelectMany(s => s.Items.OfType<PrefabItem>()).Select(p => p.Model).Distinct().OrderBy(x => x).ToList();
        int ok = 0;
        var mvTotal = 0;
        foreach (var t in tokens)
        {
            var pd = res.Load(t);
            if (pd == null) { Console.WriteLine($"  FAIL {t}: {res.FailedPpds.Last(f => f.Token == t).Error}"); continue; }
            ok++;
            mvTotal += ScsPrefab.PrefabMovements.Recover(pd, t).Count;
        }
        Console.WriteLine($"prefab 覆盖：{ok}/{tokens.Count} 加载成功，movements 共 {mvTotal}");
        return;
    }
    var pd0 = res.Load(token);
    if (pd0 == null)
    {
        Console.WriteLine($"{token} 加载失败：{res.FailedPpds.LastOrDefault(f => f.Token == token).Error}");
        return;
    }
    var mv = ScsPrefab.PrefabMovements.Recover(pd0, token);
    Console.WriteLine($"{token}: v0x{pd0.Version:x} nodes={pd0.ControlNodes.Count} curves={pd0.NavCurves.Count} navNodes={pd0.NavNodes.Count} semaphores={pd0.Semaphores.Count} movements={mv.Count}");
    Console.WriteLine($"  转向: 直行 {mv.Count(m => m.TurnType == 0)} 左 {mv.Count(m => m.TurnType == -1)} 右 {mv.Count(m => m.TurnType == 1)} U {mv.Count(m => m.TurnType == 2)}  带信号灯 {mv.Count(m => m.SemaphoreId >= 0)}");
    foreach (var m in mv.Take(8))
        Console.WriteLine($"  node {m.EntryNode} → node {m.ExitNode}：{m.CurvePath.Length} 段 {m.Length:F1}m 转向 {m.TurnType} 灯 {m.SemaphoreId}");
}

if (cmdArgs.Contains("--defs"))
{
    // P1-03 完成条件验证：sector 引用的 road type / traffic rule / lane 定义链是否全部可解析
    var res = new ScsDefinitions.DefinitionResolver(overlay);
    if (res.FailedFiles.Count > 0)
    {
        Console.WriteLine($"definition 加载失败 {res.FailedFiles.Count} 文件：");
        foreach (var (f, err) in res.FailedFiles.Take(10)) Console.WriteLine($"  FAIL {f}: {err}");
    }
    var lookRefs = sectors.SelectMany(s => s.Items.OfType<RoadItem>()).Select(r => r.RoadLook).Distinct().OrderBy(x => x).ToList();
    var ruleRefs = sectors.SelectMany(s => s.Items.OfType<RoadItem>())
        .SelectMany(r => new[] { r.RightTrafficRule, r.LeftTrafficRule }).Where(x => x.Length > 0).Distinct().OrderBy(x => x).ToList();
    var missingLooks = lookRefs.Where(x => res.GetRoadLook(x) is null).ToList();
    // road item 的 TrafficRule 字段实为 speed_class 值（限速查询键，1.60 实测：local_road/expressway/motorway）——
    // 校验其 ∈ 国家限速表 lane_speed_class 全集
    var knownSpeedClasses = res.Countries.Values
        .SelectMany(c => c.SpeedLimits.Values.SelectMany(m => m.Keys))
        .Concat(res.TrafficLanes.Values.Select(tl => tl.SpeedClass))
        .ToHashSet();
    var missingRules = ruleRefs.Where(x => !knownSpeedClasses.Contains(x)).ToList();
    // 链路：road look → lanes → traffic_lane 定义
    var missingLanes = res.RoadLooks.Values
        .SelectMany(rl => rl.LanesLeft.Concat(rl.LanesRight))
        .Distinct().Where(l => res.GetTrafficLane(l) is null).ToList();
    // 语义对象对照：Berlin 的 company/city 引用与 germany 限速存在性
    var companyRefs = sectors.SelectMany(s => s.Items.OfType<CompanyItem>()).Select(c => c.CompanyName).Distinct().OrderBy(x => x).ToList();
    var cityRefs = sectors.SelectMany(s => s.Items.OfType<CityItem>()).Select(c => c.City).Distinct().OrderBy(x => x).ToList();
    var missingCompanies = companyRefs.Where(x => res.GetCompany(x) is null).ToList();
    var missingCities = cityRefs.Where(x => res.GetCity(x) is null).ToList();
    var germany = res.GetCountry("germany");
    Console.WriteLine($"definition 统计：{res.LoadedFiles} 文件 / road_look {res.RoadLooks.Count} / traffic_lane {res.TrafficLanes.Count} / traffic_rule {res.TrafficRules.Count} / country {res.Countries.Count} / city {res.Cities.Count} / company {res.Companies.Count} / ferry {res.Ferries.Count}");
    Console.WriteLine($"sector 引用：road type {lookRefs.Count} 种，traffic rule {ruleRefs.Count} 种，company {companyRefs.Count} 种，city {cityRefs.Count} 种");
    Console.WriteLine($"缺失 road type：{missingLooks.Count}   缺失 traffic rule：{missingRules.Count}   缺失 traffic_lane 定义：{missingLanes.Count}   缺失 company：{missingCompanies.Count}   缺失 city：{missingCities.Count}   germany 限速：{(germany?.SpeedLimits.ContainsKey("car") == true ? "OK" : "缺失")}");
    foreach (var m in missingLooks.Take(10)) Console.WriteLine($"  MISSING road {m}");
    foreach (var m in missingRules.Take(10)) Console.WriteLine($"  MISSING rule {m}");
    foreach (var m in missingCompanies.Take(10)) Console.WriteLine($"  MISSING company {m}");
    foreach (var m in missingCities.Take(10)) Console.WriteLine($"  MISSING city {m}");
    var sample = res.GetRoadLook(lookRefs.FirstOrDefault() ?? "");
    if (sample != null)
        Console.WriteLine($"样本 {lookRefs.First()}: {sample.DisplayName}，车道 L={sample.LanesLeft.Count} R={sample.LanesRight.Count}");
    if (missingLooks.Count == 0 && missingRules.Count == 0 && missingCompanies.Count == 0 && missingCities.Count == 0
        && germany?.SpeedLimits.ContainsKey("car") == true)
        Console.WriteLine("P1-03 完成条件满足：Berlin 引用的 road/company/city definition 全部解析为 typed model");
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
