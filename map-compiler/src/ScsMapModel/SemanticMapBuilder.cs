using ScsResource;
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
    private readonly IScsResourceProvider? _provider;

    public SemanticMapBuilder(DefinitionResolver defs, PrefabResolver prefabs, IScsResourceProvider? provider = null)
    {
        _defs = defs;
        _prefabs = prefabs;
        _provider = provider;
    }

    /// <summary>构建语义图。Prefab 加载失败/方向未解析均不阻断（降级 + 计数）。</summary>
    public SemanticMap Build(IEnumerable<SectorFile> sectors)
    {
        var map = new SemanticMap();
        var secList = sectors.ToList();
        var allNodes = secList.SelectMany(s => s.Nodes).Select(n => n.Uid).ToHashSet();
        map.NodeCount = allNodes.Count;

        // 速度模型（country × speed_class × 城市）——road 限速计算
        var speeds = new SpeedModel(_defs, secList);
        var nodePos = secList.SelectMany(s => s.Nodes).GroupBy(n => n.Uid)
            .ToDictionary(g => g.Key, g => (g.First().X, g.First().Z));

        foreach (var sec in secList)
        {
            foreach (var road in sec.Roads)
            {
                if (!allNodes.Contains(road.Node0) || !allNodes.Contains(road.Node1)) continue;
                // speed_class：优先 road.TrafficRule（实测仅 ~4% 设置）；否则从 road look 的 lanes 推断
                // （traffic_lane 定义的 speed_class——SCS 限速实际机制）
                var speedClass = road.RightTrafficRule.Length > 0 ? road.RightTrafficRule : road.LeftTrafficRule;
                if (speedClass.Length == 0)
                {
                    var look = _defs.GetRoadLook(road.RoadLook);
                    var laneToken = look?.LanesRight.FirstOrDefault() ?? look?.LanesLeft.FirstOrDefault();
                    if (laneToken != null)
                        speedClass = _defs.GetTrafficLane(laneToken)?.SpeedClass ?? "";
                }
                // 铁路/有轨电车不作为普通道路进入路由网络（P1 收官评审 M3：P2 不得规划穿越铁轨）
                if (speedClass.StartsWith("rail", StringComparison.OrdinalIgnoreCase)) continue;
                var mid = nodePos.TryGetValue(road.Node0, out var p0) ? p0 : (0, 0);
                var sr = new SemanticRoad
                {
                    Uid = road.Uid,
                    Node0 = road.Node0,
                    Node1 = road.Node1,
                    RoadLook = road.RoadLook,
                    Direction = DetermineDirection(road, out bool degraded),
                    SpeedClass = speedClass,
                    Length = road.Length,
                    LeftHandTraffic = road.LeftHandTraffic,
                    NoAiVehicles = road.NoAiVehicles,
                    GpsAvoid = road.GpsAvoid,
                    Secret = road.Secret,
                    IsCityRoad = road.IsCityRoad,
                    DirectionDegraded = degraded,
                };
                sr.SpeedLimit = speedClass.Length > 0 ? speeds.GetSpeedLimit(mid.X, mid.Z, speedClass, road.IsCityRoad) : 0;
                map.Roads.Add(sr);
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

        // P1-10：signal group 绑定——1.60 灯配置全在 PPD 内部（prefab item 的 SemaphoreProfile 字段实测仅 1/693 设置）：
        // signal group = PPD SemaphoreId（同 id 灯同组）；组类型 = PPD 灯 Type（SemaphoreType 枚举：
        // UseProfile=0/TrafficLight=2/Minor=3/Major=4 等；UseProfile 时类型未知 → null，P2 从几何推断）
        foreach (var j in map.Junctions)
        {
            var pd = _prefabs.Load(j.PrefabToken);
            if (pd == null || pd.Semaphores.Count == 0) continue;
            var groupTypes = pd.Semaphores.GroupBy(s => s.SemaphoreId)
                .OrderBy(g => g.Key)
                .Select(g => g.First().Type switch
                {
                    0 => "use_profile",
                    1 => "model_only",
                    2 => "traffic_light",
                    3 => "traffic_light_minor",
                    4 => "traffic_light_major",
                    5 => "barrier_manual",
                    6 => "barrier_distance",
                    7 => "traffic_light_blockable",
                    8 => "barrier_gas",
                    9 => "traffic_light_virtual",
                    10 => "barrier_automatic",
                    var t => $"type_{t}",
                })
                .ToList();
            j.SignalGroupTypes = groupTypes;
            foreach (var m in j.Movements)
            {
                if (m.SemaphoreId < 0) continue;
                var groups = pd.Semaphores.Where(s => s.SemaphoreId == m.SemaphoreId).ToList();
                if (groups.Count == 0) continue;
                m.SignalGroupType = groups[0].Type switch
                {
                    3 => "traffic_light_minor",
                    4 => "traffic_light_major",
                    2 => "traffic_light",
                    _ => null,   // UseProfile/其他：类型未知（P2 从灯几何推断）
                };
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
