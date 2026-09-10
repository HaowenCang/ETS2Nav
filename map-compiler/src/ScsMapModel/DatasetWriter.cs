using System.Text;
using System.Text.Json;
using ScsGraph;
using ScsSector;

namespace ScsMapModel;

/// <summary>
/// Dataset Writer（P1 计划 §104 / ADR-004；P2-01 v2 扩展）。
/// v2（ETS2NAV_DATASET_VERSION=2）：+ edge polyline geometry table（V2-1/V2-2）、
/// + speed_limit/road_class hot metadata（V2-3/V2-4）、+ Ferry/Train 边（V2-5）、
/// + junction movement 显式 id（V2-7）。格式：magic/version/endianness + 顺序固定布局
/// （无 offset table，读取方以计数+尾部偏移校验）；可脱离 C# runtime 独立读取。
/// </summary>
public static class DatasetWriter
{
    public const uint FormatVersion = 2;
    public const string RoutingMagic = "ETS2RG1";
    public const string JunctionMagic = "ETS2JG1";

    /// <summary>speed_class 字符串 → road_class u8（0=unknown 1=local 2=expressway 3=motorway）。</summary>
    public static byte RoadClassOf(string? speedClass)
    {
        if (string.IsNullOrEmpty(speedClass)) return 0;
        if (speedClass.Contains("motorway", StringComparison.OrdinalIgnoreCase)) return 3;
        if (speedClass.Contains("express", StringComparison.OrdinalIgnoreCase)) return 2;
        if (speedClass.Contains("local", StringComparison.OrdinalIgnoreCase)) return 1;
        return 0;
    }

    // —— routing.graph v2 布局 ——
    // magic(7) + version u32(2) + endianness u32(0x12345678)
    //   + node_count u32 + edge_count u32 + geom_point_count u32
    // nodes[]:   uid u64 + x i32 + y i32 + z i32 (fixed 1/256)                    (20 B)
    // edges[]:   from u32 + to u32 + kind u8 + length f32 + source_uid u64
    //            + geom_offset u32 + geom_count u16 + speed_limit i16 + road_class u8
    //            + semaphore_id i32 + flags u8                                    (35 B)
    //            + [movement_id i32 当 flags bit3]                                (39 B)
    // geometry:  x i32 + y i32 + z i32 × geom_point_count                         (12 B/点)
    public static void WriteRoutingGraph(RoutingGraph g, string path)
    {
        using var fs = File.Create(path);
        using var w = new BinaryWriter(fs);
        w.Write(Encoding.ASCII.GetBytes(RoutingMagic));
        w.Write(FormatVersion);
        w.Write(0x12345678u);              // endianness 校验
        w.Write((uint)g.NodeCount);
        w.Write((uint)g.EdgeCount);

        // geometry table 先行（边记录引用 offset/count）
        var geom = new List<int>();        // 写入的原始 i32 点序列（x,y,z 交错）
        for (int e = 0; e < g.EdgeCount; e++) AppendEdgeGeometry(geom, g, e);
        w.Write((uint)(geom.Count / 3));

        foreach (var uid in g.NodeUids)
        {
            w.Write(uid);
            var (x, y, z) = g.Positions[g.GetNodeIndex(uid)];
            w.Write((int)Math.Round(x * 256));
            w.Write((int)Math.Round(y * 256));
            w.Write((int)Math.Round(z * 256));
        }
        int gOff = 0;
        for (int e = 0; e < g.EdgeCount; e++)
        {
            var (from, to) = g.EdgeEnds(e);
            var edge = g.Edge(e);
            int count = edge.Geometry is { Count: > 0 } ? edge.Geometry.Count : 2;   // Road 边 = 两端点直线
            w.Write((uint)from);
            w.Write((uint)to);
            w.Write((byte)edge.Kind);
            w.Write((float)edge.Length);
            w.Write(edge.SourceUid);
            w.Write((uint)(gOff * 3));              // 点偏移（单位：i32 坐标值，非点数）
            w.Write((ushort)Math.Min(count, 65535));
            w.Write((short)Math.Clamp(edge.SpeedLimitKph, short.MinValue, short.MaxValue));
            w.Write(RoadClassOf(edge.SpeedClass));
            w.Write(edge.SemaphoreId);
            byte flags = 0;
            if (edge.NoAiVehicles) flags |= 1;
            if (edge.GpsAvoid) flags |= 2;
            if (edge.Secret) flags |= 4;
            if (edge.MovementId is int mv) flags |= 8;   // 有 movement_id
            w.Write(flags);
            if (edge.MovementId is int mvId) w.Write(mvId);
            gOff += count;
        }
        foreach (int v in geom) w.Write(v);
    }

    private static void AppendEdgeGeometry(List<int> geom, RoutingGraph g, int edgeIndex)
    {
        var edge = g.Edge(edgeIndex);
        if (edge.Geometry is { Count: > 0 } pts)
        {
            foreach (var (x, y, z) in pts)
            {
                geom.Add((int)Math.Round(x * 256));
                geom.Add((int)Math.Round(y * 256));
                geom.Add((int)Math.Round(z * 256));
            }
            return;
        }
        // Road/Ferry/Train 边：两端点直线（节点坐标）
        var (from, to) = g.EdgeEnds(edgeIndex);
        var p0 = g.Positions[from];
        var p1 = g.Positions[to];
        foreach (var (x, y, z) in new[] { p0, p1 })
        {
            geom.Add((int)Math.Round(x * 256));
            geom.Add((int)Math.Round(y * 256));
            geom.Add((int)Math.Round(z * 256));
        }
    }

    // —— junction.graph v2 布局 ——
    // magic(7) + version u32(2) + endianness u32 + junction_count u32
    // junctions[]: uid u64 + prefab_token(64B 定长) + node_count u8
    //   + node_uids(u64 × node_count) + movement_count u32
    //   movements[]: id u32 + entry u64 + exit u64 + length f32 + turn i8
    //                + semaphore_id i32 + signal_group_type_len u8 + type(ASCII)
    //                + geom_offset u32 + geom_count u16                          (36 B + type)
    // geometry: x i32 + y i32 + z i32 × 总点数（movement 记录引用）
    public static void WriteJunctionGraph(SemanticMap map, string path)
    {
        using var fs = File.Create(path);
        using var w = new BinaryWriter(fs);
        w.Write(Encoding.ASCII.GetBytes(JunctionMagic));
        w.Write(FormatVersion);
        w.Write(0x12345678u);
        w.Write((uint)map.Junctions.Count);

        var geom = new List<int>();
        foreach (var j in map.Junctions)
        {
            w.Write(j.Uid);
            var token = j.PrefabToken;
            var tb = Encoding.ASCII.GetBytes(token);
            w.Write(tb);
            for (int i = tb.Length; i < 64; i++) w.Write((byte)0);
            w.Write((byte)Math.Min(j.NodeUids.Length, 255));
            for (int i = 0; i < Math.Min(j.NodeUids.Length, 255); i++) w.Write(j.NodeUids[i]);
            var validMovements = j.Movements.Where(m => m.EntryNodeUid != m.ExitNodeUid).ToList();   // 自环无导航语义
            w.Write((uint)validMovements.Count);
            int gOff = geom.Count / 3;
            foreach (var m in validMovements)
            {
                w.Write((uint)m.MovementId);            // v2：显式 id（V2-7，消除隐式索引错位）
                w.Write(m.EntryNodeUid);
                w.Write(m.ExitNodeUid);
                w.Write((float)m.Length);
                w.Write((sbyte)m.TurnType);
                w.Write(m.SemaphoreId);
                var gt = m.SignalGroupType ?? "";
                var gb = Encoding.ASCII.GetBytes(gt);
                w.Write((byte)gb.Length);
                w.Write(gb);
                var pts = m.WorldPolyline;
                int count = pts.Count >= 2 ? pts.Count : 0;   // 0 = 无几何（读取端跳过）
                w.Write((uint)(gOff * 3));              // 点偏移（单位：i32 坐标值，非字节）
                w.Write((ushort)Math.Min(count, 65535));
                gOff += Math.Max(count, 2);
            }
            // movement 几何（与记录顺序一致；无 polyline 写 0 点）
            foreach (var m in validMovements)
            {
                var pts = m.WorldPolyline;
                if (pts.Count < 2) continue;
                foreach (var (x, y, z) in pts) { geom.Add((int)Math.Round(x * 256)); geom.Add((int)Math.Round(y * 256)); geom.Add((int)Math.Round(z * 256)); }
            }
        }
        foreach (int v in geom) w.Write(v);
    }

    // —— manifest.json ——
    public static void WriteManifest(string dir, RoutingGraph g, SemanticMap map,
        IReadOnlyList<string> sectors, DateTime generatedAt, string? gameVersion = null,
        string? contentFingerprint = null)
    {
        var manifest = new
        {
            dataset_version = (int)FormatVersion,        // v2：字段名区分 format_version（P2 计划 §14）
            generated_at = generatedAt.ToString("yyyy-MM-ddTHH:mm:ssZ"),
            game_version = gameVersion,                  // P2 §29：manifest 校验（游戏版本）
            content_fingerprint = contentFingerprint,     // A4：P1 §11 安装指纹（DLC/archive 变更检测）
            scope = "europe",
            sectors = sectors,
            stats = new
            {
                nodes = g.NodeCount,
                edges = g.EdgeCount,
                roads = map.Roads.Count,
                junctions = map.Junctions.Count,
                companies = map.Companies.Count,
                cities = map.Cities.Count,
                ferries = map.Ferries.Count,
                poi = 0,
            },
            files = new[]
            {
                new { name = "routing.graph", format = RoutingMagic, version = (int)FormatVersion },
                new { name = "junction.graph", format = JunctionMagic, version = (int)FormatVersion },
                new { name = "map.db", format = "sqlite", version = 1 },
                new { name = "search.db", format = "sqlite-fts5", version = 1 },
            },
        };
        var json = JsonSerializer.Serialize(manifest, new JsonSerializerOptions { WriteIndented = true });
        File.WriteAllText(Path.Combine(dir, "manifest.json"), json);
    }

    // —— diagnostics.json ——
    public static void WriteDiagnostics(string dir, SemanticMap map, RoutingGraph g,
        IReadOnlyList<(string Token, string Error)> failedPrefabs, IReadOnlyList<string> buildNotes,
        int ferryTerminals = 0, int ferryTerminalsDegraded = 0,
        int roadsSkippedMissingNode = 0, int roadsSkippedRail = 0)
    {
        // P5 修复验收指标（2026-08-12）：transit 边端点必须落在陆地路网上
        // （至少有一条非 transit 边）——否则 ferry/train 无法桥接路网。
        var landNodes = new HashSet<ulong>();
        var transitNodes = new HashSet<ulong>();
        for (int e = 0; e < g.EdgeCount; e++)
        {
            var (f, t) = g.EdgeEnds(e);
            var fu = g.NodeUids[f];
            var tu = g.NodeUids[t];
            if (g.Edge(e).Kind is RoutingEdgeKind.Ferry or RoutingEdgeKind.Train)
            {
                transitNodes.Add(fu);
                transitNodes.Add(tu);
            }
            else
            {
                landNodes.Add(fu);
                landNodes.Add(tu);
            }
        }
        var isolatedTransit = transitNodes.Count(u => !landNodes.Contains(u));
        var diag = new
        {
            roads = new { total = map.Roads.Count, one_way = map.Roads.Count(r => r.Direction is RoadDirection.ForwardOnly or RoadDirection.BackwardOnly), degraded = map.Roads.Count(r => r.DirectionDegraded), skipped_missing_node = roadsSkippedMissingNode, skipped_rail = roadsSkippedRail },
            junctions = new { total = map.Junctions.Count, no_movement = map.Junctions.Count(j => j.Movements.Count == 0) },
            movements = new { total = map.Junctions.Sum(j => j.Movements.Count), with_semaphore = map.Junctions.SelectMany(j => j.Movements).Count(m => m.SemaphoreId >= 0), with_geometry = map.Junctions.SelectMany(j => j.Movements).Count(m => m.WorldPolyline.Count >= 2) },
            ferries = new { total = map.Ferries.Count, one_way = map.Ferries.Count(f => f.AtoB != f.BtoA), terminals = ferryTerminals, terminals_degraded = ferryTerminalsDegraded, transit_nodes = transitNodes.Count, transit_nodes_isolated = isolatedTransit },
            companies = new { total = map.Companies.Count, no_access = map.Companies.Count(c => c.AccessNodeUid is null) },
            graph = new { nodes = g.NodeCount, edges = g.EdgeCount, transit_edges = g.Edges.Count(e => e.Kind is RoutingEdgeKind.Ferry or RoutingEdgeKind.Train) },
            failed_prefabs = failedPrefabs.Select(f => new { token = f.Token, error = f.Error }).ToList(),
            notes = buildNotes,
        };
        var json = JsonSerializer.Serialize(diag, new JsonSerializerOptions { WriteIndented = true });
        File.WriteAllText(Path.Combine(dir, "diagnostics.json"), json);
    }
}
