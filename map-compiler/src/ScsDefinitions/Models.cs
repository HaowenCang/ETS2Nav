namespace ScsDefinitions;

/// <summary>Road Look 定义（P1 计划 §15，最优先 definition）——来自 /def/world/road_look.sii。</summary>
public sealed class RoadLookDefinition
{
    public required string Name { get; init; }          // road.look0
    public string? DisplayName { get; init; }           // name: "Road 1 lane double"
    public double RoadSizeLeft { get; init; }
    public double RoadSizeRight { get; init; }
    public double RoadOffset { get; init; }
    public double ShoulderSizeLeft { get; init; }
    public double ShoulderSizeRight { get; init; }
    public List<string> LanesLeft { get; } = new();     // traffic_lane.xxx token
    public List<string> LanesRight { get; } = new();

    /// <summary>线型样式编号（0=none, 1=solid, 2=dashed, 3=double 等；corpus 对照语义，见格式笔记）。</summary>
    public int CenterLineLeftStyle { get; init; }
    public int CenterLineRightStyle { get; init; }
    public int InnerLineStyle { get; init; }
    public int OuterLineStyle { get; init; }

    public int LaneCount => LanesLeft.Count + LanesRight.Count;
}

/// <summary>Traffic Lane 定义——来自 /def/world/traffic_lane.sii。</summary>
public sealed class TrafficLaneDefinition
{
    public required string Name { get; init; }          // traffic_lane.road.local
    public string SpeedClass { get; init; } = "local_road";   // local_road/expressway/motorway/...
    public int Rank { get; init; }
    public List<string> TrafficRules { get; } = new();

    /// <summary>允许在对向车道超车（traffic_rule.overtake_alw）→ 支持双向单车道路段的反向通行。</summary>
    public bool AllowsOvertake => TrafficRules.Contains("traffic_rule.overtake_alw");
}

/// <summary>Traffic Rule 定义——来自 /def/world/traffic_rules.sii（traffic_lane 的 traffic_rules[] 同命名空间）。</summary>
public sealed class TrafficRuleDefinition
{
    public required string Name { get; init; }          // traffic_rule.road / traffic_rule.overtake_alw
    public string? SpeedClass { get; init; }
    public int Rank { get; init; }
}

/// <summary>国家限速条目（country_speed_limit 的并行数组展开，P1 计划 §16）。</summary>
public sealed class CountrySpeedLimit
{
    public int Limit { get; init; }
    public int UrbanLimit { get; init; }
    public int MaxLimit { get; init; }
}

/// <summary>国家定义——/def/country/&lt;name&gt;/speed_limits.sii（+ traffic 等基础）。</summary>
public sealed class CountryDefinition
{
    public required string Name { get; init; }          // germany
    /// <summary>vehicle_speed_class（car/truck/bus…）→ lane_speed_class（local_road/expressway/motorway…）→ 限速。</summary>
    public Dictionary<string, Dictionary<string, CountrySpeedLimit>> SpeedLimits { get; } = new();
}

/// <summary>城市定义——/def/city/*.sui。</summary>
public sealed class CityDefinition
{
    public required string Name { get; init; }          // berlin（city_data : city.berlin）
    public string? DisplayName { get; init; }           // city_name_localized（@@本地化@@ 或直文）
    public string? Country { get; init; }
    public int Population { get; init; }
}

/// <summary>公司定义——/def/company/*.sui（company_permanent）。</summary>
public sealed class CompanyDefinition
{
    public required string Name { get; init; }          // company.permanent.acc
    public string? DisplayName { get; init; }
    public string? SortName { get; init; }
}

/// <summary>渡口/隧道定义——/def/ferry/*.sui。DisplayName 可能为本地化键（@@xx@@），与 city 一致。</summary>
public sealed class FerryDefinition
{
    public required string Name { get; init; }          // ferry.calais 等
    public string? DisplayName { get; init; }
}
