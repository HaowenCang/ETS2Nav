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
    // 城市 country 区域（CityItem 的 Width/Height 为城市矩形范围——判定用 bbox 而非半径圆，
    // 避免跨边境误判（P1 收官评审 M5：柏林东北角 6.3% road 曾被 szczecin 半径误判为 poland））
    private readonly List<(double X, double Z, double HalfW, double HalfH, string Country)> _cityCountries = new();
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
                _cityCountries.Add((n.X, n.Z, Math.Max(c.Width / 2, 800), Math.Max(c.Height / 2, 800), country));
            }
        }
    }

    /// <summary>road 所在国家（城市 bbox 判定；无城市回退默认）。</summary>
    public string CountryAt(double x, double z)
    {
        string best = _defaultCountry;
        double bestDist = double.MaxValue;
        foreach (var (cx, cz, hw, hh, country) in _cityCountries)
        {
            if (Math.Abs(x - cx) > hw || Math.Abs(z - cz) > hh) continue;   // bbox 外
            double d = Math.Abs(x - cx) + Math.Abs(z - cz);                  // L1 距离（bbox 内排序）
            if (d < bestDist) { bestDist = d; best = country; }
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
        if (cd is null) return -1;                                   // 未知国家（无限速表）
        if (!cd.SpeedLimits.TryGetValue(vehicleClass, out var byLane)) return -1;
        if (!byLane.TryGetValue(speedClass, out var lim)) return -1;  // 未知 speed_class
        return isCity ? lim.UrbanLimit : lim.Limit;                  // 0 = 无限速（autobahn 等）
    }
}
