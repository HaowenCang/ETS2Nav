namespace ScsMapModel;

/// <summary>道路允许通行方向（P1 计划 §15：判断 A→B / B→A / A↔B）。</summary>
public enum RoadDirection { None, ForwardOnly, BackwardOnly, Both }

/// <summary>Semantic Map（P1 计划 §18）：SCS 格式对象 → 导航语义对象。</summary>
public sealed class SemanticMap
{
    public List<SemanticRoad> Roads { get; } = new();
    public List<SemanticJunction> Junctions { get; } = new();
    public List<SemanticCompany> Companies { get; } = new();
    public List<SemanticCity> Cities { get; } = new();

    public int NodeCount { get; set; }
}

/// <summary>道路段（P1 计划 §19）：连接两个节点的一段道路，方向来自 road look 车道语义。</summary>
public sealed class SemanticRoad
{
    public required ulong Uid { get; init; }
    public required ulong Node0 { get; init; }
    public required ulong Node1 { get; init; }
    public required string RoadLook { get; init; }
    public required RoadDirection Direction { get; init; }
    /// <summary>限速等级（road item 的 TrafficRule 字段 = speed_class 值）。</summary>
    public required string SpeedClass { get; init; }
    /// <summary>限速 km/h（SpeedModel 计算；0 = 无限速）。</summary>
    public int SpeedLimit { get; set; }
    public double Length { get; init; }
    public bool LeftHandTraffic { get; init; }
    public bool NoAiVehicles { get; init; }
    public bool GpsAvoid { get; init; }
    public bool Secret { get; init; }
    public bool IsCityRoad { get; init; }
    /// <summary>road look 未解析时的方向降级标记。</summary>
    public bool DirectionDegraded { get; init; }
}

/// <summary>prefab 实例 = 路口（P1 计划 §21 Junction）。</summary>
public sealed class SemanticJunction
{
    public required ulong Uid { get; init; }          // prefab item uid
    public required string PrefabToken { get; init; }
    public required string SemaphoreProfile { get; init; }
    /// <summary>信号组类型序列（profile type[]：traffic_light_major/minor…，P1-10）。</summary>
    public IReadOnlyList<string> SignalGroupTypes { get; set; } = Array.Empty<string>();
    public ulong[] NodeUids { get; init; } = Array.Empty<ulong>();
    public List<JunctionMovement> Movements { get; } = new();
    public bool LeftHandTraffic { get; init; }
}

/// <summary>路口 movement（P1 计划 §21）：prefab navigation 语义允许的 entry→exit。</summary>
public sealed class JunctionMovement
{
    public required int MovementId { get; init; }
    public required ulong EntryNodeUid { get; init; }
    public required ulong ExitNodeUid { get; init; }
    public double Length { get; init; }
    /// <summary>-1 左转 / 0 直行 / 1 右转 / 2 U 型。</summary>
    public required int TurnType { get; init; }
    public int SemaphoreId { get; init; } = -1;
    /// <summary>signal group 类型（junction.SignalGroupTypes[SemaphoreId % count]；无 profile 时 null）。</summary>
    public string? SignalGroupType { get; set; }
    public int PriorityModifier { get; init; }
    public bool LowProbability { get; init; }
    /// <summary>PPD 曲线链（prefab 内部几何，供 Junction Graph）。</summary>
    public int[] CurvePath { get; init; } = Array.Empty<int>();
}

/// <summary>公司（P1 计划 §25）：visual position + routing access（linked prefab 的入口节点）。</summary>
public sealed class SemanticCompany
{
    public required ulong Uid { get; init; }
    public required string CompanyName { get; init; }
    public required ulong LinkedPrefabUid { get; init; }
    /// <summary>routing access 节点（linked prefab 的任一 movement 入口）。</summary>
    public ulong? AccessNodeUid { get; set; }
}

/// <summary>城市区域。</summary>
public sealed class SemanticCity
{
    public required ulong Uid { get; init; }
    public required string CityToken { get; init; }
    public ulong[] NodeUids { get; init; } = Array.Empty<ulong>();
}
