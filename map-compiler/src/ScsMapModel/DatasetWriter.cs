using System.Text;
using System.Text.Json;
using ScsGraph;
using ScsSector;

namespace ScsMapModel;

/// <summary>
/// Dataset Writer（P1 计划 §104 / ADR-004）：routing.graph/junction.graph 紧凑二进制 + manifest.json。
/// 格式：magic/version/endianness + 顺序固定布局（无 offset table，读取方以计数+尾部偏移校验）；可脱离 C# runtime 独立读取。
/// </summary>
public static class DatasetWriter
{
    public const uint FormatVersion = 1;
    public const string RoutingMagic = "ETS2RG1";
    public const string JunctionMagic = "ETS2JG1";

    // —— routing.graph 二进制布局 ——
    // magic(7) + version u32 + endianness u32(0x12345678) + node_count u32 + edge_count u32
    // nodes[]: uid u64 + x i32(fixed 1/256) + y i32 + z i32                     (20 B)
    // edges[]: from u32 + to u32 + kind u8 + length f32 + source_uid u64
    //          + semaphore_id i32 + flags u8                                     (26 B)
    public static void WriteRoutingGraph(RoutingGraph g, string path)
    {
        using var fs = File.Create(path);
        using var w = new BinaryWriter(fs);
        w.Write(Encoding.ASCII.GetBytes(RoutingMagic));
        w.Write(FormatVersion);
        w.Write(0x12345678u);              // endianness 校验
        w.Write((uint)g.NodeCount);
        w.Write((uint)g.EdgeCount);
        foreach (var uid in g.NodeUids)
        {
            w.Write(uid);
            var (x, y, z) = g.Positions[g.GetNodeIndex(uid)];
            w.Write((int)Math.Round(x * 256));
            w.Write((int)Math.Round(y * 256));
            w.Write((int)Math.Round(z * 256));
        }
        for (int e = 0; e < g.EdgeCount; e++)
        {
            var (from, to) = g.EdgeEnds(e);
            var edge = g.Edge(e);
            w.Write((uint)from);
            w.Write((uint)to);
            w.Write((byte)edge.Kind);
            w.Write((float)edge.Length);
            w.Write(edge.SourceUid);
            w.Write(edge.SemaphoreId);
            byte flags = 0;
            if (edge.NoAiVehicles) flags |= 1;
            if (edge.GpsAvoid) flags |= 2;
            if (edge.Secret) flags |= 4;
            if (edge.MovementId is int mv) flags |= 8;   // 有 movement_id
            w.Write(flags);
            if (edge.MovementId is int mvId) w.Write(mvId);
        }
    }

    // —— junction.graph 二进制布局 ——
    // magic(7) + version u32 + endianness u32 + junction_count u32
    // junctions[]: uid u64 + prefab_token(64B 定长) + node_count u8 + movement_count u32
    //   node_uids[]: u64 × node_count
    //   movements[]: entry u64 + exit u64 + length f32 + turn i8 + semaphore_id i32 + signal_group_type_len u8 + type(ASCII)
    public static void WriteJunctionGraph(SemanticMap map, string path)
    {
        using var fs = File.Create(path);
        using var w = new BinaryWriter(fs);
        w.Write(Encoding.ASCII.GetBytes(JunctionMagic));
        w.Write(FormatVersion);
        w.Write(0x12345678u);
        w.Write((uint)map.Junctions.Count);
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
            foreach (var m in validMovements)
            {
                w.Write(m.EntryNodeUid);
                w.Write(m.ExitNodeUid);
                w.Write((float)m.Length);
                w.Write((sbyte)m.TurnType);
                w.Write(m.SemaphoreId);
                var gt = m.SignalGroupType ?? "";
                var gb = Encoding.ASCII.GetBytes(gt);
                w.Write((byte)gb.Length);
                w.Write(gb);
            }
        }
    }

    // —— manifest.json ——
    public static void WriteManifest(string dir, RoutingGraph g, SemanticMap map,
        IReadOnlyList<string> sectors, DateTime generatedAt)
    {
        var manifest = new
        {
            format_version = FormatVersion,
            generated_at = generatedAt.ToString("yyyy-MM-ddTHH:mm:ssZ"),
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
        IReadOnlyList<(string Token, string Error)> failedPrefabs, IReadOnlyList<string> buildNotes)
    {
        var diag = new
        {
            roads = new { total = map.Roads.Count, one_way = map.Roads.Count(r => r.Direction is RoadDirection.ForwardOnly or RoadDirection.BackwardOnly), degraded = map.Roads.Count(r => r.DirectionDegraded) },
            junctions = new { total = map.Junctions.Count, no_movement = map.Junctions.Count(j => j.Movements.Count == 0) },
            movements = new { total = map.Junctions.Sum(j => j.Movements.Count), with_semaphore = map.Junctions.SelectMany(j => j.Movements).Count(m => m.SemaphoreId >= 0) },
            companies = new { total = map.Companies.Count, no_access = map.Companies.Count(c => c.AccessNodeUid is null) },
            graph = new { nodes = g.NodeCount, edges = g.EdgeCount },
            failed_prefabs = failedPrefabs.Select(f => new { token = f.Token, error = f.Error }).ToList(),
            notes = buildNotes,
        };
        var json = JsonSerializer.Serialize(diag, new JsonSerializerOptions { WriteIndented = true });
        File.WriteAllText(Path.Combine(dir, "diagnostics.json"), json);
    }
}
