// 验证器实现（P1 §36–40）。从 P0 GraphValidator 拆分并正式化：
// Structural（自环/重复边/重复 UID/节点引用完整性）→ Structural + Reference；
// Direction（方向一致性）；Connectivity（分量统计）；Geometry（零长边/NaN/teleport）。
// GPL-3.0 — ETS2Nav 项目

using ScsGraph;
using ScsSector;

namespace ScsValidation.Validators;

/// <summary>结构检测（P1 §36）：自环、重复边、重复 UID、断链引用。</summary>
public sealed class StructuralValidator : IGraphValidator
{
    public string Name => "structural";

    public void Validate(ValidationContext ctx, List<ValidationIssue> sink)
    {
        // 重复 UID（跨 sector item 集合）
        var uidSeen = new HashSet<ulong>();
        foreach (var sec in ctx.Sectors)
        {
            foreach (var item in sec.Items)
            {
                if (!uidSeen.Add(item.Uid))
                    sink.Add(new ValidationIssue
                    {
                        Code = "STRUCT_DUP_UID",
                        Severity = ValidationSeverity.Error,
                        Description = $"item UID {item.Uid:x16} 重复",
                        SourceUid = item.Uid,
                        Sector = sec.SectorName,
                    });
            }
        }

        // 自环 + 重复边（图级）
        var seen = new HashSet<(int From, int To, ulong Item)>();
        for (int u = 0; u < ctx.Graph.NodeCount; u++)
        {
            foreach (int e in ctx.Graph.OutEdges(u))
            {
                var (from, to) = ctx.Graph.EdgeEnds(e);
                var data = ctx.Graph.Edge(e);
                if (from == to)
                    sink.Add(new ValidationIssue
                    {
                        Code = "STRUCT_SELF_LOOP",
                        Severity = ValidationSeverity.Error,
                        Description = $"边自环（node {from} == {to}）",
                        SourceUid = data.ItemUid,
                    });
                if (!seen.Add((from, to, data.ItemUid)))
                    sink.Add(new ValidationIssue
                    {
                        Code = "STRUCT_DUP_EDGE",
                        Severity = ValidationSeverity.Warning,
                        Description = $"重复边 {from}->{to}",
                        SourceUid = data.ItemUid,
                    });
            }
        }

        // missing node（§36）：road/prefab 引用的节点必须存在于节点表
        var nodeUids = new HashSet<ulong>();
        foreach (var sec in ctx.Sectors)
            foreach (var n in sec.Nodes)
                nodeUids.Add(n.Uid);
        foreach (var sec in ctx.Sectors)
        {
            foreach (var road in sec.Roads)
            {
                if (!nodeUids.Contains(road.Node0))
                    sink.Add(new ValidationIssue
                    {
                        Code = "STRUCT_MISSING_NODE",
                        Severity = ValidationSeverity.Error,
                        Description = $"road 引用不存在的节点 {road.Node0:x16}",
                        SourceUid = road.Uid,
                        Sector = sec.SectorName,
                    });
                if (!nodeUids.Contains(road.Node1))
                    sink.Add(new ValidationIssue
                    {
                        Code = "STRUCT_MISSING_NODE",
                        Severity = ValidationSeverity.Error,
                        Description = $"road 引用不存在的节点 {road.Node1:x16}",
                        SourceUid = road.Uid,
                        Sector = sec.SectorName,
                    });
            }
            foreach (var pf in sec.Prefabs)
                foreach (var nu in pf.NodeUids)
                    if (!nodeUids.Contains(nu))
                        sink.Add(new ValidationIssue
                        {
                            Code = "STRUCT_MISSING_NODE",
                            Severity = ValidationSeverity.Error,
                            Description = $"prefab 引用不存在的节点 {nu:x16}",
                            SourceUid = pf.Uid,
                            Sector = sec.SectorName,
                        });
        }
    }
}

/// <summary>引用完整性（P1 §36）：node 的 backward/forward item 必须存在。</summary>
public sealed class ReferenceValidator : IGraphValidator
{
    public string Name => "reference";

    public void Validate(ValidationContext ctx, List<ValidationIssue> sink)
    {
        var itemUids = new HashSet<ulong>();
        foreach (var sec in ctx.Sectors)
            foreach (var item in sec.Items)
                itemUids.Add(item.Uid);
        foreach (var sec in ctx.Sectors)
        {
            foreach (var n in sec.Nodes)
            {
                if (n.BackwardItemUid != 0 && !itemUids.Contains(n.BackwardItemUid))
                    sink.Add(new ValidationIssue
                    {
                        Code = "REF_BROKEN_BACKWARD",
                        Severity = ValidationSeverity.Warning,
                        Description = $"node 引用 backward item {n.BackwardItemUid:x16} 不在已加载 sector 集（可能位于未加载 sector）",
                        SourceUid = n.Uid,
                        Sector = sec.SectorName,
                    });
                if (n.ForwardItemUid != 0 && !itemUids.Contains(n.ForwardItemUid))
                    sink.Add(new ValidationIssue
                    {
                        Code = "REF_BROKEN_FORWARD",
                        Severity = ValidationSeverity.Warning,
                        Description = $"node 引用 forward item {n.ForwardItemUid:x16} 不在已加载 sector 集（可能位于未加载 sector）",
                        SourceUid = n.Uid,
                        Sector = sec.SectorName,
                    });
            }
        }
    }
}

/// <summary>方向一致性（P1 §37）：road 的 node0/node1 与 node 的 forward/backward item 一致。</summary>
public sealed class DirectionValidator : IGraphValidator
{
    public string Name => "direction";

    public void Validate(ValidationContext ctx, List<ValidationIssue> sink)
    {
        // 全局节点表（跨 sector 构建，P1-01 评审 m2）
        var nodeByUid = new Dictionary<ulong, MapNode>();
        foreach (var sec in ctx.Sectors)
            foreach (var n in sec.Nodes)
                nodeByUid.TryAdd(n.Uid, n);
        foreach (var sec in ctx.Sectors)
        {
            foreach (var road in sec.Roads)
            {
                if (nodeByUid.TryGetValue(road.Node0, out var n0) && n0.ForwardItemUid != 0 && n0.ForwardItemUid != road.Uid)
                    sink.Add(new ValidationIssue
                    {
                        Code = "DIR_NODE0_MISMATCH",
                        Severity = ValidationSeverity.Error,
                        Description = $"road Node0 的 forward item 是 {n0.ForwardItemUid:x16} 而非自身",
                        SourceUid = road.Uid,
                        Sector = sec.SectorName,
                    });
                if (nodeByUid.TryGetValue(road.Node1, out var n1) && n1.BackwardItemUid != 0 && n1.BackwardItemUid != road.Uid)
                    sink.Add(new ValidationIssue
                    {
                        Code = "DIR_NODE1_MISMATCH",
                        Severity = ValidationSeverity.Error,
                        Description = $"road Node1 的 backward item 是 {n1.BackwardItemUid:x16} 而非自身",
                        SourceUid = road.Uid,
                        Sector = sec.SectorName,
                    });
            }
        }
    }
}

/// <summary>连通性（P1 §40）：连通分量统计、孤立节点、最大分量占比。</summary>
public sealed class ConnectivityValidator : IGraphValidator
{
    public string Name => "connectivity";

    /// <summary>连通性统计（P1 §40）：分量/孤立/死端限定在边端点节点集（非道路 item 节点不计入，P1-01 评审 M2）。</summary>
    public static (int Components, int LargestNodes, int Isolated, int DeadEnds, int NonRoadNodes) Compute(RoadGraph graph)
    {
        int n = graph.NodeCount;
        // 边端点集（有边参与的节点）
        var touched = new bool[n];
        var degree = new int[n];
        for (int e = 0; e < graph.EdgeCount; e++)
        {
            var (from, to) = graph.EdgeEnds(e);
            touched[from] = true; touched[to] = true;
            degree[from]++;
            degree[to]++;
        }
        int nonRoad = n - touched.Count(t => t);
        var comp = new int[n];
        Array.Fill(comp, -1);
        int compCount = 0, largest = 0, isolated = 0, deadEnds = 0;
        for (int s = 0; s < n; s++)
        {
            if (!touched[s] || comp[s] >= 0) continue;
            int size = 0;
            var stack = new Stack<int>();
            stack.Push(s);
            comp[s] = compCount;
            while (stack.Count > 0)
            {
                int u = stack.Pop();
                size++;
                foreach (int e in graph.OutEdges(u))
                {
                    var (_, to) = graph.EdgeEnds(e);
                    if (comp[to] < 0) { comp[to] = compCount; stack.Push(to); }
                }
            }
            if (size == 1) isolated++;
            if (size > largest) largest = size;
            compCount++;
        }
        for (int u = 0; u < n; u++)
            if (touched[u] && degree[u] == 1) deadEnds++;
        return (compCount, largest, isolated, deadEnds, nonRoad);
    }

    public void Validate(ValidationContext ctx, List<ValidationIssue> sink)
    {
        if (ctx.Graph.NodeCount == 0) return;
        var (compCount, largest, isolated, deadEnds, nonRoad) = Compute(ctx.Graph);
        int roadNodes = ctx.Graph.NodeCount - nonRoad;
        sink.Add(new ValidationIssue
        {
            Code = "CONN_STATS",
            Severity = ValidationSeverity.Info,
            Description = $"连通分量 {compCount}，最大分量 {largest} 节点（道路子图 {100.0 * largest / Math.Max(1, roadNodes):F1}%），" +
                          $"孤立节点 {isolated}，死端 {deadEnds}，非道路 item 节点 {nonRoad}（不计入分量）",
        });
        if (isolated > 0)
            sink.Add(new ValidationIssue
            {
                Code = "CONN_ISOLATED",
                Severity = ValidationSeverity.Info,
                Description = $"{isolated} 个孤立道路节点（仅含自环或无通路）",
            });
    }
}

/// <summary>几何检测（P1 §39）：零长边、NaN 坐标、异常 teleport。</summary>
public sealed class GeometryValidator : IGraphValidator
{
    public string Name => "geometry";

    public void Validate(ValidationContext ctx, List<ValidationIssue> sink)
    {
        var positions = ctx.Graph.Positions;
        for (int e = 0; e < ctx.Graph.EdgeCount; e++)
        {
            var (from, to) = ctx.Graph.EdgeEnds(e);
            var (x0, y0, z0) = positions[from];
            var (x1, y1, z1) = positions[to];
            if (double.IsNaN(x0) || double.IsNaN(z0) || double.IsNaN(x1) || double.IsNaN(z1))
            {
                sink.Add(new ValidationIssue
                {
                    Code = "GEOM_NAN",
                    Severity = ValidationSeverity.Error,
                    Description = $"边 {e} 含 NaN 坐标",
                    SourceUid = ctx.Graph.Edge(e).ItemUid,
                });
                continue;
            }
            double dx = x1 - x0, dz = z1 - z0;
            double len = Math.Sqrt(dx * dx + dz * dz);
            if (len < 0.01)
                sink.Add(new ValidationIssue
                {
                    Code = "GEOM_ZERO_LENGTH",
                    Severity = ValidationSeverity.Warning,
                    Description = $"零长边 {e}（({x0:F1},{z0:F1})==({x1:F1},{z1:F1})）",
                    SourceUid = ctx.Graph.Edge(e).ItemUid,
                });
            else if (len > 2000)
                sink.Add(new ValidationIssue
                {
                    Code = "GEOM_TELEPORT",
                    Severity = ValidationSeverity.Warning,
                    Description = $"疑似 teleport 边 {e}：长度 {len:F0}m（合法道路边上限 ≈1000m）",
                    SourceUid = ctx.Graph.Edge(e).ItemUid,
                });
        }
    }
}
