using ScsDefinitions;
using ScsPrefab;
using ScsSector;

namespace ScsMapModel;

/// <summary>
/// Semantic Map Builder（P1 计划 §32 正式链路）：SectorFile → SemanticMap。
/// road 方向来自 road look 车道语义；prefab 连通性完全来自 navigation movements。
/// </summary>
public sealed class SemanticMapBuilder
{
    private readonly DefinitionResolver _defs;
    private readonly PrefabResolver _prefabs;

    public SemanticMapBuilder(DefinitionResolver defs, PrefabResolver prefabs)
    {
        _defs = defs;
        _prefabs = prefabs;
    }

    /// <summary>构建语义图。Prefab 加载失败/方向未解析均不阻断（降级 + 计数）。</summary>
    public SemanticMap Build(IEnumerable<SectorFile> sectors)
    {
        var map = new SemanticMap();
        var secList = sectors.ToList();
        var allNodes = secList.SelectMany(s => s.Nodes).Select(n => n.Uid).ToHashSet();
        map.NodeCount = allNodes.Count;

        foreach (var sec in secList)
        {
            foreach (var road in sec.Roads)
            {
                if (!allNodes.Contains(road.Node0) || !allNodes.Contains(road.Node1)) continue;
                map.Roads.Add(new SemanticRoad
                {
                    Uid = road.Uid,
                    Node0 = road.Node0,
                    Node1 = road.Node1,
                    RoadLook = road.RoadLook,
                    Direction = DetermineDirection(road, out bool degraded),
                    SpeedClass = road.RightTrafficRule.Length > 0 ? road.RightTrafficRule : road.LeftTrafficRule,
                    Length = road.Length,
                    LeftHandTraffic = road.LeftHandTraffic,
                    NoAiVehicles = road.NoAiVehicles,
                    GpsAvoid = road.GpsAvoid,
                    Secret = road.Secret,
                    IsCityRoad = road.IsCityRoad,
                    DirectionDegraded = degraded,
                });
            }
            foreach (var city in sec.Items.OfType<CityItem>())
                map.Cities.Add(new SemanticCity { Uid = city.Uid, CityToken = city.City });
        }

        // prefab → junction（movement 节点映射：(curveNode + Origin) % N → NodeUids）
        foreach (var sec in secList)
        {
            foreach (var pf in sec.Prefabs)
            {
                var pd = _prefabs.Load(pf.Model);
                var j = new SemanticJunction
                {
                    Uid = pf.Uid,
                    PrefabToken = pf.Model,
                    SemaphoreProfile = pf.SemaphoreProfile,
                    NodeUids = pf.NodeUids,
                    LeftHandTraffic = pf.LeftHandTraffic,
                };
                if (pd != null)
                {
                    var movements = PrefabMovements.Recover(pd, pf.Model);
                    int n = pf.NodeUids.Length;
                    for (int i = 0; i < movements.Count; i++)
                    {
                        var m = movements[i];
                        j.Movements.Add(new JunctionMovement
                        {
                            MovementId = i,
                            EntryNodeUid = MapControlNode(m.EntryNode, pf.OriginIndex, n, pf.NodeUids),
                            ExitNodeUid = MapControlNode(m.ExitNode, pf.OriginIndex, n, pf.NodeUids),
                            Length = m.Length,
                            TurnType = m.TurnType,
                            SemaphoreId = m.SemaphoreId,
                            PriorityModifier = m.PriorityModifier,
                            LowProbability = m.LowProbability,
                            CurvePath = m.CurvePath,
                        });
                    }
                }
                map.Junctions.Add(j);
            }
        }

        // 有边节点集（road 端点 + 有 movement 的 junction 节点）——公司 access 判定用
        var roadTouched = new HashSet<ulong>();
        foreach (var sec in secList)
        {
            foreach (var r in sec.Roads) { roadTouched.Add(r.Node0); roadTouched.Add(r.Node1); }
        }
        foreach (var j in map.Junctions)
            if (j.Movements.Count > 0)
                foreach (var m in j.Movements) { roadTouched.Add(m.EntryNodeUid); roadTouched.Add(m.ExitNodeUid); }

        // 公司：routing access = linked prefab 节点中第一个有边节点（公司 prefab 自身无导航曲线——
        // P1 计划 §25：access 必须位于合法道路入口；无道路连接的标记 null 待 P1-08）
        foreach (var sec in secList)
        {
            foreach (var c in sec.Items.OfType<CompanyItem>())
            {
                var j = map.Junctions.FirstOrDefault(x => x.Uid == c.LinkedPrefabUid);
                var company = new SemanticCompany
                {
                    Uid = c.Uid,
                    CompanyName = c.CompanyName,
                    LinkedPrefabUid = c.LinkedPrefabUid,
                };
                if (j != null)
                {
                    company.AccessNodeUid = j.NodeUids.FirstOrDefault(n => roadTouched.Contains(n));
                }
                map.Companies.Add(company);
            }
        }
        return map;
    }

    /// <summary>PPD ControlNode 索引 → prefab NodeUids（ETS2LA 语义：index - Origin 轮转）。</summary>
    private static ulong MapControlNode(byte controlIndex, ushort origin, int nodeCount, ulong[] nodeUids)
    {
        if (nodeCount == 0) return 0;
        int idx = (controlIndex - origin) % nodeCount;
        if (idx < 0) idx += nodeCount;
        return nodeUids[idx];
    }

    /// <summary>road 方向判定：lanes_left/right 车道分布（P1 计划 §15）。
    /// 仅右车道 → node0→node1；仅左车道 → node1→node0；双侧 → 双向。
    /// road look 未解析 → 双向降级（DirectionDegraded）。</summary>
    private RoadDirection DetermineDirection(RoadItem road, out bool degraded)
    {
        degraded = false;
        var look = _defs.GetRoadLook(road.RoadLook);
        if (look is null)
        {
            degraded = true;
            return RoadDirection.Both;   // 未知 look：保守双向
        }
        bool left = look.LanesLeft.Count > 0;
        bool right = look.LanesRight.Count > 0;
        if (right && !left) return RoadDirection.ForwardOnly;   // node0 → node1
        if (left && !right) return RoadDirection.BackwardOnly;  // node1 → node0
        return RoadDirection.Both;
    }
}
