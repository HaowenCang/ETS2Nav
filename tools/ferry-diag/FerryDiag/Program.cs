// ferry-diag：ferry/train 码头接入诊断（P5 断簇排查）。
// 目的：验证 FerryItem.NodeUid 是否接入路网、PrefabLinkUid 是否可解析为可接入的 prefab。
// 用法：ferry-diag --dir <解包根> --sectors a,b,c
//      ferry-diag --install <游戏根> --region europe   （全量，慢）
// GPL-3.0 — ETS2Nav 项目

using ScsSector;
using ScsResource;
using ScsGraph;

var cmdArgs = Environment.GetCommandLineArgs().Skip(1).ToArray();
string? installDir = Arg(cmdArgs, "--install");
string dir = Arg(cmdArgs, "--dir") ?? @"E:\Projects\Pi\ETS2Nav\vendor\extracted";
string mapPrefix = installDir != null ? "/map/europe/" : "/base_map/map/europe/";

using OverlayProvider overlay = installDir != null
    ? GameInstall.Detect(installDir).BuildOverlay()
    : new OverlayProvider(new DirectoryProvider(dir));

var secNames = (Arg(cmdArgs, "--sectors") ?? "").Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
if (cmdArgs.Contains("--all-sectors"))
{
    secNames = overlay.Enumerate(installDir != null ? "/map/europe" : "/base_map/map/europe")
        .Where(p => p.EndsWith(".base") && (p.Contains("/sec+") || p.Contains("/sec-")))
        .Select(p => Path.GetFileNameWithoutExtension(p)!)
        .OrderBy(n => n)
        .ToArray();
}

var sectors = new List<SectorFile>();
foreach (var name in secNames)
{
    foreach (var ext in new[] { ".base", ".aux" })
    {
        var vp = $"{mapPrefix}{name}{ext}";
        if (!overlay.Exists(vp)) continue;
        using var s = overlay.Open(vp);
        sectors.Add(SectorFile.Read(s, name));
    }
}
Console.WriteLine($"DIAG sectors={sectors.Count} items={sectors.Sum(s => s.Items.Count)} nodes={sectors.Sum(s => s.Nodes.Count)}");

var defs = new ScsDefinitions.DefinitionResolver(overlay);
var prefabs = new ScsPrefab.PrefabResolver(overlay);
var map = new ScsMapModel.SemanticMapBuilder(defs, prefabs, overlay).Build(sectors);
var rg = ScsMapModel.RoutingGraphBuilder.Build(map, sectors);

// 路由图中全部节点 uid 及是否有出/入边
var hasEdge = new HashSet<ulong>();
for (int e = 0; e < rg.EdgeCount; e++)
{
    var (f, t) = rg.EdgeEnds(e);
    hasEdge.Add(rg.NodeUids[f]);
    hasEdge.Add(rg.NodeUids[t]);
}
// 非 transit 边端点（陆地路网）
var landNodes = new HashSet<ulong>();
for (int e = 0; e < rg.EdgeCount; e++)
{
    var edge = rg.Edge(e);
    if (edge.Kind is RoutingEdgeKind.Ferry or RoutingEdgeKind.Train) continue;
    var (f, t) = rg.EdgeEnds(e);
    landNodes.Add(rg.NodeUids[f]);
    landNodes.Add(rg.NodeUids[t]);
}
Console.WriteLine($"DIAG routing nodes={rg.NodeCount} edges={rg.EdgeCount} nodes_with_any_edge={hasEdge.Count} land_nodes={landNodes.Count}");
var (comps, largest, noEdge) = rg.ConnectedComponents();
Console.WriteLine($"DIAG components={comps} largest={largest} no_edge_nodes={noEdge}");

// 节点坐标查表
var posOf = new Dictionary<ulong, (double X, double Y, double Z)>();
for (int i = 0; i < rg.NodeCount; i++) posOf[rg.NodeUids[i]] = rg.Positions[i];
// 陆地节点坐标（用于最近邻）
var landPos = landNodes.Where(posOf.ContainsKey).Select(u => (Uid: u, P: posOf[u])).ToList();

int n = 0;
foreach (var sec in sectors)
{
    foreach (var f in sec.Items.OfType<FerryItem>())
    {
        n++;
        var ferryNode = f.NodeUid;
        bool inGraph = posOf.ContainsKey(ferryNode);
        bool anyEdge = hasEdge.Contains(ferryNode);
        bool isLand = landNodes.Contains(ferryNode);
        var fp = inGraph ? posOf[ferryNode] : (0.0, 0.0, 0.0);

        // PrefabLinkUid 是否解析到 junction
        var linked = map.Junctions.FirstOrDefault(j => j.Uid == f.PrefabLinkUid);
        // 该 ferry 节点是否属于某 prefab 的节点表
        var owner = map.Junctions.FirstOrDefault(j => j.NodeUids.Contains(ferryNode));

        // 最近陆地节点
        double best = double.MaxValue;
        ulong bestUid = 0;
        foreach (var (u, p) in landPos)
        {
            double dx = p.X - fp.Item1, dz = p.Z - fp.Item3;
            double d = dx * dx + dz * dz;
            if (d < best) { best = d; bestUid = u; }
        }
        double bestM = Math.Sqrt(best);

        Console.WriteLine(
            $"FERRY sector={sec.SectorName} port={f.Port} train={f.IsTrain} node={ferryNode:x16} pos=({fp.Item1:F0},{fp.Item3:F0}) " +
            $"inGraph={inGraph} anyEdge={anyEdge} isLandNode={isLand} " +
            $"prefabLink={f.PrefabLinkUid:x16} linkResolved={(linked != null)} linkToken={(linked?.PrefabToken ?? "-")} " +
            $"linkNodes={(linked?.NodeUids.Length.ToString() ?? "-")} linkLandNodes={(linked == null ? "-" : linked.NodeUids.Count(u => landNodes.Contains(u)).ToString())} " +
            $"ownerPrefab={(owner?.Uid.ToString("x16") ?? "-")} ownerToken={(owner?.PrefabToken ?? "-")} ownerMovements={(owner?.Movements.Count.ToString() ?? "-")} " +
            $"nearestLand={bestM:F0}m");

        if (linked != null && n <= 10)
        {
            foreach (var u in linked.NodeUids)
            {
                var p = posOf.TryGetValue(u, out var pp) ? pp : (0.0, 0.0, 0.0);
                Console.WriteLine($"    LINKNODE uid={u:x16} pos=({p.Item1:F0},{p.Item3:F0}) land={landNodes.Contains(u)} anyEdge={hasEdge.Contains(u)}");
            }
        }
        // 该节点入边/出边（判是否有道路汇入）
        int outCnt = 0, inCnt = 0;
        if (inGraph)
        {
            int idx = rg.GetNodeIndex(ferryNode);
            outCnt = rg.OutEdges(idx).Count;
            for (int e = 0; e < rg.EdgeCount; e++) if (rg.EdgeEnds(e).To == idx) inCnt++;
        }
        if (n <= 10) Console.WriteLine($"    FERRYNODE out={outCnt} in={inCnt}");
    }
}
// ---- prefab 连通性探针（--prefab-conn <token>）：检查指定 prefab 实例的节点间
// ---- 是否由道路连通，以及 0 曲线 prefab 是否应提供连通性 ----
var connToken = Arg(cmdArgs, "--prefab-conn");
if (connToken != null)
{
    var roadByNode = new Dictionary<ulong, List<(ulong Other, string Uid, double Len)>>();
    foreach (var sec in sectors)
    {
        foreach (var r in sec.Roads)
        {
            roadByNode.TryAdd(r.Node0, new());
            roadByNode.TryAdd(r.Node1, new());
            roadByNode[r.Node0].Add((r.Node1, r.Uid.ToString("x16"), r.Length));
            roadByNode[r.Node1].Add((r.Node0, r.Uid.ToString("x16"), r.Length));
        }
    }
    var uidPos = new Dictionary<ulong, (double X, double Y, double Z)>();
    foreach (var sec in sectors) foreach (var nd in sec.Nodes) uidPos[nd.Uid] = (nd.X, nd.Y, nd.Z);

    int shown = 0;
    // 直接按 prefab item 找实例
    foreach (var sec in sectors)
    {
        foreach (var pf in sec.Prefabs)
        {
            if (pf.Model != connToken) continue;
            if (shown++ >= 5) break;
            var pd = prefabs.Load(pf.Model);
            var mv = pd == null ? new List<ScsPrefab.PrefabMovement>() : ScsPrefab.PrefabMovements.Recover(pd, pf.Model);
            Console.WriteLine(
                $"PCONN sector={sec.SectorName} prefab={pf.Model} uid={pf.Uid:x16} origin={pf.OriginIndex} " +
                $"nodes={pf.NodeUids.Length} ppdCurves={(pd?.NavCurves.Count ?? -1)} movements={mv.Count} left={pf.LeftHandTraffic}");
            for (int i = 0; i < pf.NodeUids.Length; i++)
            {
                var u = pf.NodeUids[i];
                var inRg = posOf.ContainsKey(u);
                var p = uidPos.TryGetValue(u, out var pp) ? pp : (X: 0.0, Y: 0.0, Z: 0.0);
                var roads = roadByNode.TryGetValue(u, out var rl) ? rl : new();
                int landN = 0, compN = 0;
                if (inRg)
                {
                    landN = landNodes.Contains(u) ? 1 : 0;
                    var idx = rg.GetNodeIndex(u);
                    compN = rg.OutEdges(idx).Count;
                }
                var others = string.Join(" ", roads.Take(4).Select(x => $"{x.Other.ToString("x16")[..8]}({x.Len:F0}m)"));
                Console.WriteLine(
                    $"  n{i} uid={u:x16} pos=({p.X:F1},{p.Z:F1}) inGraph={inRg} landNode={landN} outEdges={compN} roads={roads.Count} [{others}]");
            }
        }
    }
    Console.WriteLine($"PCONN instances_shown={shown}");
    return 0;
}

// ---- movement 恢复完备性检验（--mvcomplete）----
// 判据：PPD 内 Physical(ControlNode) 节点在 NavNode 连通图上的分量划分，应被
// PrefabMovements.Recover 的 movement 连通划分**细化或相等**（movement 是子集关系：
// 每个 movement 是 nav 图内一条路径 ⇒ movement 连通 ⊆ nav 图连通）。
// 若 movement 连通比 nav 图连通更粗（分量更少）⇒ 存在 nav 图可达、movement 未覆盖
// 的 Physical 节点对 ⇒ 恢复算法漏恢复（P5 残余断簇的候选成因）。
var mvComplete = cmdArgs.Contains("--mvcomplete");
if (mvComplete)
{
    var tokens = sectors.SelectMany(s => s.Prefabs).Select(p => p.Model).Distinct().OrderBy(x => x).ToList();
    int checkedCount = 0, underRecovered = 0, noMovement = 0, depthLimited = 0;
    var samples = new List<string>();
    foreach (var t in tokens)
    {
        var pd = prefabs.Load(t);
        if (pd == null) continue;
        checkedCount++;
        // nav 图：NavNode 索引 → 邻接
        int nn = pd.NavNodes.Count;
        var adj = new List<List<int>>(nn);
        for (int i = 0; i < nn; i++) adj.Add(new List<int>());
        for (int i = 0; i < nn; i++)
            foreach (var c in pd.NavNodes[i].Connections)
                if (c.TargetNodeIndex < nn && c.CurveIndices.Any(x => x < pd.NavCurves.Count))
                    adj[i].Add(c.TargetNodeIndex);
        // Physical 节点：navNode 索引 → controlNode 索引
        var phys = new Dictionary<int, int>();
        for (int i = 0; i < nn; i++)
            if (pd.NavNodes[i].Type == 0 && pd.NavNodes[i].Index < pd.ControlNodes.Count)
                phys[i] = pd.NavNodes[i].Index;
        if (phys.Count < 2) continue;
        // nav 图连通划分（限定在 Physical 节点集合上：i,j 同簇 ⟺ nav 图中连通）
        var ufNav = new int[nn];
        for (int i = 0; i < nn; i++) ufNav[i] = i;
        int Find(int[] uf, int x) { while (uf[x] != x) { uf[x] = uf[uf[x]]; x = uf[x]; } return x; }
        for (int i = 0; i < nn; i++)
            foreach (var j in adj[i])
            {
                int a = Find(ufNav, i), b = Find(ufNav, j);
                if (a != b) ufNav[a] = b;
            }
        var navRoots = phys.Keys.Select(k => Find(ufNav, k)).ToHashSet();
        // BFS 深度：Physical 节点对在 nav 图上的最短跳数（判定 MaxDepth 是否截断）
        bool limited = false;
        foreach (var start in phys.Keys)
        {
            var dist = new int[nn];
            for (int i = 0; i < nn; i++) dist[i] = -1;
            dist[start] = 0;
            var q = new Queue<int>();
            q.Enqueue(start);
            while (q.Count > 0)
            {
                int u = q.Dequeue();
                foreach (var v in adj[u])
                    if (dist[v] < 0) { dist[v] = dist[u] + 1; q.Enqueue(v); }
            }
            foreach (var end in phys.Keys)
                if (end != start && dist[end] >= 16) limited = true;
        }
        if (limited) depthLimited++;
        // movement 恢复后的连通划分
        var mv = ScsPrefab.PrefabMovements.Recover(pd, t);
        var ufMv = new int[nn];
        for (int i = 0; i < nn; i++) ufMv[i] = i;
        // controlNode 索引 → navNode 索引（反查）
        var ctrlToNav = phys.ToDictionary(kv => kv.Value, kv => kv.Key);
        foreach (var m in mv)
        {
            if (ctrlToNav.TryGetValue(m.EntryNode, out int a2) && ctrlToNav.TryGetValue(m.ExitNode, out int b2))
            {
                int ra = Find(ufMv, a2), rb = Find(ufMv, b2);
                if (ra != rb) ufMv[ra] = rb;
            }
        }
        int mvClusters = phys.Keys.Select(k => Find(ufMv, k)).ToHashSet().Count;
        if (mv.Count == 0) noMovement++;
        if (mvClusters > navRoots.Count)
        {
            underRecovered++;
            if (samples.Count < 12)
                samples.Add($"token={t} physNodes={phys.Count} navClusters={navRoots.Count} mvClusters={mvClusters} movements={mv.Count} depthLimited={limited}");
        }
    }
    Console.WriteLine($"MVCOMPLETE tokens_checked={checkedCount} under_recovered={underRecovered} zero_movement={noMovement} depth_limited_ge16={depthLimited}");
    foreach (var s in samples) Console.WriteLine("  MVCOMPLETE-SAMPLE " + s);
    return underRecovered == 0 ? 0 : 1;
}

// ---- 原始转储（--dump-raw）：与本项目解析器的 item 级转储，供 oracle-conn 逐项 diff ----
// 格式与 oracle-conn 一致：R/P/F 行，uid 升序。
if (cmdArgs.Contains("--dump-raw"))
{
    var rows = new List<(ulong Uid, string Line)>();
    foreach (var sec in sectors)
    {
        foreach (var r in sec.Roads)
            rows.Add((r.Uid, $"R {r.Uid:x16} {r.Node0:x16} {r.Node1:x16}"));
        foreach (var pf in sec.Prefabs)
            rows.Add((pf.Uid, $"P {pf.Uid:x16} {string.Join(' ', pf.NodeUids.Select(n => n.ToString("x16")))}"));
        foreach (var f in sec.Items.OfType<ScsSector.FerryItem>())
            rows.Add((f.Uid, $"F {f.Uid:x16} {f.NodeUid:x16} {f.Port} {f.IsTrain}"));
    }
    foreach (var row in rows.OrderBy(x => x.Uid)) Console.WriteLine(row.Line);
    Console.Error.WriteLine(
        $"ours: roads={rows.Count(r => r.Line[0] == 'R')} prefabs={rows.Count(r => r.Line[0] == 'P')} ferries={rows.Count(r => r.Line[0] == 'F')}");
    return 0;
}

// ---- 连接缺失检验（--linkcheck <uidA> <uidB>）：两个近邻节点间是否存在道路/prefab 关系 ----
var lc = cmdArgs.Contains("--linkcheck") ? cmdArgs.SkipWhile(a => a != "--linkcheck").Skip(1).Take(2).ToArray() : Array.Empty<string>();
if (lc.Length == 2)
{
    var ua = Convert.ToUInt64(lc[0], 16);
    var ub = Convert.ToUInt64(lc[1], 16);
    Console.WriteLine($"LINKCHECK a={ua:x16} b={ub:x16}");
    // 1) 是否有 road 直接连接二者
    int direct = 0;
    foreach (var sec in sectors)
        foreach (var r in sec.Roads)
            if ((r.Node0 == ua && r.Node1 == ub) || (r.Node0 == ub && r.Node1 == ua))
            {
                direct++;
                Console.WriteLine($"  ROAD-DIRECT sector={sec.SectorName} uid={r.Uid:x16} look={r.RoadLook} len={r.Length:F1}");
            }
    Console.WriteLine($"  direct_roads={direct}");
    // 2) 二者各自的全部道路邻接
    foreach (var (label, u) in new[] { ("A", ua), ("B", ub) })
    {
        var list = new List<string>();
        foreach (var sec in sectors)
            foreach (var r in sec.Roads)
            {
                if (r.Node0 == u) list.Add($"{sec.SectorName}:{r.Node1:x16}");
                else if (r.Node1 == u) list.Add($"{sec.SectorName}:{r.Node0:x16}");
            }
        Console.WriteLine($"  {label} uid={u:x16} road_adjacency={list.Count} [{string.Join(" ", list.Take(6))}]");
    }
    // 3) 二者是否同属某 prefab
    foreach (var sec in sectors)
        foreach (var pf in sec.Prefabs)
        {
            bool ha = pf.NodeUids.Contains(ua), hb = pf.NodeUids.Contains(ub);
            if (ha || hb)
                Console.WriteLine($"  PREFAB sector={sec.SectorName} model={pf.Model} uid={pf.Uid:x16} nodes={pf.NodeUids.Length} hasA={ha} hasB={hb}");
        }
    // 4) 二者是否在任何 sector 的节点表中
    foreach (var (label, u) in new[] { ("A", ua), ("B", ub) })
    {
        var where = sectors.Where(s => s.Nodes.Any(x => x.Uid == u)).Select(s => s.SectorName).ToList();
        Console.WriteLine($"  {label} defined_in_sectors=[{string.Join(",", where)}]");
    }
    return 0;
}

Console.WriteLine($"DIAG ferries_total={n} map_ferries={map.Ferries.Count}");

// ---- transit 边端点连通性（P5 关键判据：端点必须有非 transit 边，否则 ferry 不桥接路网） ----
int transitEdges = 0, endpointsTotal = 0, endpointsIsolated = 0;
Console.WriteLine("TRANSIT-EDGES:");
for (int e = 0; e < rg.EdgeCount; e++)
{
    var edge = rg.Edge(e);
    if (edge.Kind is not (RoutingEdgeKind.Ferry or RoutingEdgeKind.Train)) continue;
    transitEdges++;
    var (f, t) = rg.EdgeEnds(e);
    var fu = rg.NodeUids[f];
    var tu = rg.NodeUids[t];
    bool fl = landNodes.Contains(fu), tl = landNodes.Contains(tu);
    endpointsTotal += 2;
    if (!fl) endpointsIsolated++;
    if (!tl) endpointsIsolated++;
    if (transitEdges <= 12)
    {
        var pf = posOf.TryGetValue(fu, out var a) ? a : (0.0, 0.0, 0.0);
        var pt = posOf.TryGetValue(tu, out var b) ? b : (0.0, 0.0, 0.0);
        Console.WriteLine(
            $"  TE edge={e} kind={edge.Kind} len={edge.Length:F0} " +
            $"from={fu:x16} pos=({pf.Item1:F0},{pf.Item3:F0}) land={fl} | to={tu:x16} pos=({pt.Item1:F0},{pt.Item3:F0}) land={tl}");
    }
}
Console.WriteLine($"TRANSIT-SUMMARY edges={transitEdges} endpoints={endpointsTotal} endpoints_isolated={endpointsIsolated}");
return endpointsIsolated == 0 && transitEdges > 0 ? 0 : 1;

static string? Arg(string[] a, string key)
{
    for (int i = 0; i < a.Length - 1; i++) if (a[i] == key) return a[i + 1];
    return null;
}
