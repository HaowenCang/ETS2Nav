using ScsGraph;
using ScsSector;

namespace ScsMapModel;

/// <summary>
/// Routing Graph Builder（P1 计划 §32）：SemanticMap → RoutingGraph。
/// Road 边按方向生成（单向只一条）；JunctionMovement 边 = prefab navigation 语义（无全连接）。
/// </summary>
public static class RoutingGraphBuilder
{
    public static RoutingGraph Build(SemanticMap map, IEnumerable<SectorFile> sectors)
    {
        var g = new RoutingGraph();
        var secList = sectors.ToList();

        // 节点：全部 sector MapNode
        foreach (var sec in secList)
            foreach (var n in sec.Nodes)
                g.EnsureNode(n.Uid, n.X, n.Y, n.Z);

        // Road 边：按方向
        foreach (var r in map.Roads)
        {
            if (!g.TryGetNodeIndex(r.Node0, out int a) || !g.TryGetNodeIndex(r.Node1, out int b)) continue;
            var baseEdge = new RoutingEdge
            {
                Kind = RoutingEdgeKind.Road,
                SourceUid = r.Uid,
                Length = r.Length,
                RoadLook = r.RoadLook,
                SpeedClass = r.SpeedClass,
                NoAiVehicles = r.NoAiVehicles,
                GpsAvoid = r.GpsAvoid,
                Secret = r.Secret,
            };
            if (r.Direction is RoadDirection.ForwardOnly or RoadDirection.Both) g.AddDirected(a, b, baseEdge);
            if (r.Direction is RoadDirection.BackwardOnly or RoadDirection.Both) g.AddDirected(b, a, baseEdge);
        }

        // JunctionMovement 边：prefab navigation movements（entry → exit）
        foreach (var j in map.Junctions)
        {
            foreach (var m in j.Movements)
            {
                if (!g.TryGetNodeIndex(m.EntryNodeUid, out int a) || !g.TryGetNodeIndex(m.ExitNodeUid, out int b)) continue;
                if (a == b) continue;   // 自环防御
                g.AddDirected(a, b, new RoutingEdge
                {
                    Kind = RoutingEdgeKind.JunctionMovement,
                    SourceUid = j.Uid,
                    Length = m.Length,
                    MovementId = m.MovementId,
                    SemaphoreId = m.SemaphoreId,
                });
            }
        }

        return g;
    }
}
