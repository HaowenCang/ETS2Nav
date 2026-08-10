using ScsDefinitions;
using ScsSector;

namespace ScsMapModel;

/// <summary>
/// 速度模型（P1 计划 §102）：road → 限速。
/// 输入：country 限速表（P1-03 DefinitionResolver.Countries）+ road speed_class + 城市 flag。
/// country 空间判定：road 坐标 → 最近城市（≤50km）→ 城市 country；无城市回退默认国家。
/// </summary>
public sealed class SpeedModel
{
    private readonly List<(double X, double Z, string Country, double Radius)> _cityCountries = new();
    private readonly string _defaultCountry;
    private readonly DefinitionResolver _defs;

    public SpeedModel(DefinitionResolver defs, IEnumerable<SectorFile> sectors, string defaultCountry = "germany")
    {
        _defs = defs;
        _defaultCountry = defaultCountry;
        foreach (var sec in sectors)
        {
            foreach (var c in sec.Items.OfType<CityItem>())
            {
                var n = sec.Nodes.FirstOrDefault(x => x.Uid == c.NodeUid);
                if (n is null) continue;
                // 城市 country：city token → defs.Cities（key 可能带 city. 前缀）
                var city = defs.GetCity(c.City);
                var country = city?.Country ?? _defaultCountry;
                _cityCountries.Add((n.X, n.Z, country, Math.Max(c.Width, c.Height) * 2 + 5000));
            }
        }
    }

    /// <summary>road 所在国家（最近城市判定；无城市回退默认）。</summary>
    public string CountryAt(double x, double z)
    {
        string best = _defaultCountry;
        double bestDist = double.MaxValue;
        foreach (var (cx, cz, country, radius) in _cityCountries)
        {
            double d = Math.Sqrt((x - cx) * (x - cx) + (z - cz) * (z - cz));
            if (d < bestDist && d <= radius) { bestDist = d; best = country; }
        }
        return best;
    }

    /// <summary>
    /// 计算限速（km/h）。0 = 无限速（autobahn 等）。
    /// vehicleClass 默认 truck（卡车限速——导航目标车辆）。
    /// </summary>
    public int GetSpeedLimit(double x, double z, string speedClass, bool isCity, string vehicleClass = "truck")
    {
        var country = CountryAt(x, z);
        return GetSpeedLimit(country, speedClass, isCity, vehicleClass);
    }

    public int GetSpeedLimit(string country, string speedClass, bool isCity, string vehicleClass = "truck")
    {
        var cd = _defs.GetCountry(country);
        if (cd is null) return 0;
        if (!cd.SpeedLimits.TryGetValue(vehicleClass, out var byLane)) return 0;
        if (!byLane.TryGetValue(speedClass, out var lim)) return 0;
        return isCity ? lim.UrbanLimit : lim.Limit;
    }
}
