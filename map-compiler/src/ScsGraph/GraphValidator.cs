// Graph Validation（v0.2 §17 基础：结构检测 + 方向检测雏形）。
// 结构检测：自环、重复边、节点引用完整性、死端统计。
// 方向检测：利用 MapNode.BackwardItemUid/ForwardItemUid 与 Road 方向的一致性。
// MIT License — ETS2Nav 项目

using ScsSector;

namespace ScsGraph;

public sealed class ValidationReport
{
    public List<string> SelfLoops { get; } = new();          // road uid: node0 == node1
    public List<string> DuplicateEdges { get; } = new();     // 同 from/to 同 item uid 重复
    public List<string> BrokenNodeRefs { get; } = new();     // node 引用的 item 不存在
    public List<string> DirectionMismatches { get; } = new();// road 方向与 node forward/backward 矛盾
    public int DeadEnds { get; set; }                        // degree==1 道路节点数
    public int PrefabOnlyComponents { get; set; }            // 仅含 prefab 边的分量（无道路接入）

    public bool HasErrors =>
        SelfLoops.Count > 0 || DuplicateEdges.Count > 0 || BrokenNodeRefs.Count > 0
        || DirectionMismatches.Count > 0;
}

public static class GraphValidator
{
    /// <summary>对 sector 集合 + 构建的图执行验证。</summary>
    public static ValidationReport Validate(IEnumerable<SectorFile> sectors, RoadGraph graph)
    {
        var report = new ValidationReport();
        var sectorsList = sectors.ToList();

        // 1) 自环 + 重复边
        var seen = new HashSet<(int From, int To, ulong Item)>();
        for (int u = 0; u < graph.NodeCount; u++)
        {
            foreach (int e in graph.OutEdges(u))
            {
                var (from, to) = graph.EdgeEnds(e);
                var data = graph.Edge(e);
                if (from == to) report.SelfLoops.Add($"{data.ItemUid:x16}");
                if (!seen.Add((from, to, data.ItemUid)))
                    report.DuplicateEdges.Add($"{data.ItemUid:x16} ({from}->{to})");
            }
        }

        // 2) 节点引用完整性：node 的 backward/forward item 必须存在于 item 集合
        var itemUids = new HashSet<ulong>();
        foreach (var sec in sectorsList)
            foreach (var item in sec.Items)
                itemUids.Add(item.Uid);
        foreach (var sec in sectorsList)
        {
            foreach (var n in sec.Nodes)
            {
                if (n.BackwardItemUid != 0 && !itemUids.Contains(n.BackwardItemUid))
                    report.BrokenNodeRefs.Add($"node {n.Uid:x16} 引用不存在的 backward item {n.BackwardItemUid:x16}");
                if (n.ForwardItemUid != 0 && !itemUids.Contains(n.ForwardItemUid))
                    report.BrokenNodeRefs.Add($"node {n.Uid:x16} 引用不存在的 forward item {n.ForwardItemUid:x16}");
            }
        }

        // 3) 方向一致性：road 的 node0/node1 与 node 的 forward/backward item
        //    数据模型：road.Node0 = item 起点（该节点上 item 为 forward）；
        //    road.Node1 = item 终点（该节点上 item 为 backward）。
        foreach (var sec in sectorsList)
        {
            var nodeByUid = new Dictionary<ulong, MapNode>();
            foreach (var n in sec.Nodes) nodeByUid[n.Uid] = n;
            foreach (var road in sec.Roads)
            {
                if (nodeByUid.TryGetValue(road.Node0, out var n0) && n0.ForwardItemUid != 0 && n0.ForwardItemUid != road.Uid)
                    report.DirectionMismatches.Add($"road {road.Uid:x16} Node0 的 forward item 是 {n0.ForwardItemUid:x16} 而非自身");
                if (nodeByUid.TryGetValue(road.Node1, out var n1) && n1.BackwardItemUid != 0 && n1.BackwardItemUid != road.Uid)
                    report.DirectionMismatches.Add($"road {road.Uid:x16} Node1 的 backward item 是 {n1.BackwardItemUid:x16} 而非自身");
            }
        }

        // 4) 死端统计：degree==1 的道路节点
        for (int u = 0; u < graph.NodeCount; u++)
            if (graph.OutEdges(u).Count == 1) report.DeadEnds++;

        return report;
    }
}
