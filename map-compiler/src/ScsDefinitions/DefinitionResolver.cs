using ScsResource;
using ScsSii;

namespace ScsDefinitions;

/// <summary>
/// Definition Resolver（P1 计划 §14）：raw SII → strongly typed model。
/// 预加载官方 def corpus 的关键 definition 文件（含 @include 展开），
/// 提供按 token 名的查询。上层（Graph Builder）不得直接查询 SiiUnit.Attributes。
/// </summary>
public sealed class DefinitionResolver
{
    private readonly Dictionary<string, RoadLookDefinition> _roadLooks = new();
    private readonly Dictionary<string, TrafficLaneDefinition> _trafficLanes = new();
    private readonly Dictionary<string, CountryDefinition> _countries = new();
    private readonly Dictionary<string, CityDefinition> _cities = new();
    private readonly Dictionary<string, CompanyDefinition> _companies = new();
    private readonly Dictionary<string, FerryDefinition> _ferries = new();

    public IReadOnlyDictionary<string, RoadLookDefinition> RoadLooks => _roadLooks;
    public IReadOnlyDictionary<string, TrafficLaneDefinition> TrafficLanes => _trafficLanes;
    public IReadOnlyDictionary<string, CountryDefinition> Countries => _countries;
    public IReadOnlyDictionary<string, CityDefinition> Cities => _cities;
    public IReadOnlyDictionary<string, CompanyDefinition> Companies => _companies;
    public IReadOnlyDictionary<string, FerryDefinition> Ferries => _ferries;

    /// <summary>加载的 definition 文件数（诊断）。</summary>
    public int LoadedFiles { get; private set; }

    public DefinitionResolver(IScsResourceProvider provider) => LoadAll(provider);

    /// <summary>road type 引用解析：sector 存无前缀名（at1），definition unit 名为 road.xxx——
    /// 先尝试 "road." 前缀，再回退裸名（同 ETS2LA 解析逻辑）。</summary>
    public RoadLookDefinition? GetRoadLook(string name)
        => _roadLooks.TryGetValue("road." + name, out var v) ? v
         : _roadLooks.TryGetValue(name, out var w) ? w : null;
    public TrafficLaneDefinition? GetTrafficLane(string name) => _trafficLanes.TryGetValue(name, out var v) ? v : null;
    public CountryDefinition? GetCountry(string name) => _countries.TryGetValue(name, out var v) ? v : null;
    public CityDefinition? GetCity(string name)
        => _cities.TryGetValue(name, out var v) ? v
         : _cities.TryGetValue("city." + name, out var w) ? w : null;
    public CompanyDefinition? GetCompany(string name)
        => _companies.TryGetValue(name, out var v) ? v
         : _companies.TryGetValue("company.permanent." + name, out var w) ? w : null;
    public FerryDefinition? GetFerry(string name) => _ferries.TryGetValue(name, out var v) ? v : null;

    private void LoadAll(IScsResourceProvider p)
    {
        var files = new List<(string Path, string? Country)>
        {
            ("/def/world/road_look.sii", null),
            ("/def/world/traffic_lane.sii", null),
            ("/def/city.sii", null),
            ("/def/ferry.sii", null),
        };
        // 1.60 模板化 road look：road_look.template*.sii（含 road.xxx 模板 look 与 tmpl_var）
        files.AddRange(p.Enumerate("/def/world")
            .Where(x => x.EndsWith(".sii") && x.Contains("road_look"))
            .Select(x => (x, (string?)null)));
        foreach (var x in p.Enumerate("/def/company").Where(x => x.EndsWith(".sui")))
            files.Add((x, null));
        // /def/country/<name>/speed_limits.sii → country = 目录名
        foreach (var x in p.Enumerate("/def/country").Where(x => x.EndsWith(".sii")))
        {
            var segs = x.Split('/', StringSplitOptions.RemoveEmptyEntries);
            files.Add((x, segs.Length >= 3 ? segs[2] : null));
        }
        foreach (var (f, country) in files)
        {
            SiiDocument doc;
            try { doc = DefinitionLoader.Load(p, f); }
            catch { continue; }     // 单个 definition 文件损坏不阻断整体（诊断计数仍加）
            LoadedFiles++;
            foreach (var u in doc.Units) RouteUnit(u, country);
        }
    }

    private void RouteUnit(SiiUnit u, string? country)
    {
        switch (u.Class)
        {
            case "road_look":
                _roadLooks[u.Name] = new RoadLookDefinition
                {
                    Name = u.Name,
                    DisplayName = Str(u, "name"),
                    RoadSizeLeft = Num(u, "road_size_left"),
                    RoadSizeRight = Num(u, "road_size_right"),
                    RoadOffset = Num(u, "road_offset"),
                    ShoulderSizeLeft = Num(u, "shoulder_size_left"),
                    ShoulderSizeRight = Num(u, "shoulder_size_right"),
                    CenterLineLeftStyle = Int(u, "center_line_left_style"),
                    CenterLineRightStyle = Int(u, "center_line_right_style"),
                    InnerLineStyle = Int(u, "inner_line_style"),
                    OuterLineStyle = Int(u, "outer_line_style"),
                }.Also(rl =>
                {
                    foreach (var v in u.Values("lanes_left[]")) if (v.Kind == SiiValueKind.Token) rl.LanesLeft.Add(v.Str!);
                    foreach (var v in u.Values("lanes_right[]")) if (v.Kind == SiiValueKind.Token) rl.LanesRight.Add(v.Str!);
                });
                break;
            case "traffic_lane_data":
                _trafficLanes[u.Name] = new TrafficLaneDefinition
                {
                    Name = u.Name,
                    SpeedClass = Str(u, "speed_class") ?? "local_road",
                    Rank = Int(u, "rank"),
                }.Also(tl =>
                {
                    foreach (var v in u.Values("traffic_rules[]")) if (v.Kind == SiiValueKind.Token) tl.TrafficRules.Add(v.Str!);
                });
                break;
            case "country_speed_limit":
                if (country != null)
                {
                    if (!_countries.TryGetValue(country, out var cd))
                        _countries[country] = cd = new CountryDefinition { Name = country };
                    RegisterCountryLimit(cd, u);
                }
                break;
            case "city_data":
                _cities[u.Name] = new CityDefinition
                {
                    Name = u.Name,
                    DisplayName = Str(u, "city_name_localized") ?? Str(u, "city_name"),
                    Country = Str(u, "country"),
                    Population = Int(u, "population"),
                };
                break;
            case "company_permanent":
                _companies[u.Name] = new CompanyDefinition
                {
                    Name = u.Name,
                    DisplayName = Str(u, "name"),
                    SortName = Str(u, "sort_name"),
                };
                break;
            default:
                if (u.Class.EndsWith("ferry_data"))
                    _ferries[u.Name] = new FerryDefinition { Name = u.Name, DisplayName = Str(u, "name") ?? Str(u, "ferry_name") };
                break;
        }
    }

    private static void RegisterCountryLimit(CountryDefinition cd, SiiUnit u)
    {
        // 并行数组展开：lane_speed_class[] 为锚，同索引取 limit[]/urban_limit[]/max_limit[]
        var lanes = Tokens(u, "lane_speed_class[]");
        var limits = Nums(u, "limit[]");
        var urbans = Nums(u, "urban_limit[]");
        var maxes = Nums(u, "max_limit[]");
        var vehicle = Str(u, "vehicle_speed_class") ?? "default";
        if (!cd.SpeedLimits.TryGetValue(vehicle, out var laneMap))
            cd.SpeedLimits[vehicle] = laneMap = new Dictionary<string, CountrySpeedLimit>();
        for (int i = 0; i < lanes.Count; i++)
        {
            laneMap[lanes[i]] = new CountrySpeedLimit
            {
                Limit = i < limits.Count ? (int)limits[i] : 0,
                UrbanLimit = i < urbans.Count ? (int)urbans[i] : 0,
                MaxLimit = i < maxes.Count ? (int)maxes[i] : 0,
            };
        }
    }

    // —— SiiUnit 值辅助 ——
    internal static string? Str(SiiUnit u, string key)
    {
        var hit = u.Attributes.FirstOrDefault(a => a.Key == key);
        return hit.Value is { Kind: SiiValueKind.String or SiiValueKind.Token } ? hit.Value.Str : null;
    }

    internal static int Int(SiiUnit u, string key)
        => u.Attributes.FirstOrDefault(a => a.Key == key).Value is { Kind: SiiValueKind.Number } v ? (int)v.Num : 0;

    internal static double Num(SiiUnit u, string key)
        => u.Attributes.FirstOrDefault(a => a.Key == key).Value is { Kind: SiiValueKind.Number } v ? v.Num : 0;

    internal static List<string> Tokens(SiiUnit u, string key)
        => u.Values(key).Where(v => v.Kind == SiiValueKind.Token).Select(v => v.Str!).ToList();

    internal static List<double> Nums(SiiUnit u, string key)
        => u.Values(key).Where(v => v.Kind == SiiValueKind.Number).Select(v => v.Num).ToList();
}

internal static class DefExtensions
{
    public static T Also<T>(this T obj, Action<T> fn) { fn(obj); return obj; }
}
