using ScsDefinitions;
using ScsResource;
using Xunit;

namespace ScsDefinitions.Tests;

/// <summary>内存目录 provider（测试用）：写文件 → DirectoryProvider 读取。</summary>
public class DefResolverTests : IDisposable
{
    private readonly string _root;
    private readonly OverlayProvider _overlay;

    public DefResolverTests()
    {
        _root = Path.Combine(Path.GetTempPath(), "ets2nav-def-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(_root, "def", "world"));
        Directory.CreateDirectory(Path.Combine(_root, "def", "country", "germany"));
        Directory.CreateDirectory(Path.Combine(_root, "def", "city"));
        Directory.CreateDirectory(Path.Combine(_root, "def", "company"));
        _overlay = new OverlayProvider(new DirectoryProvider(_root));
    }

    public void Dispose() { _overlay.Dispose(); Directory.Delete(_root, true); }

    private void Write(string rel, string content)
    {
        var p = Path.Combine(_root, rel.Replace('/', Path.DirectorySeparatorChar));
        Directory.CreateDirectory(Path.GetDirectoryName(p)!);
        File.WriteAllText(p, content);
    }

    [Fact]
    public void RoadLook_WithPrefixFallback()
    {
        Write("def/world/road_look.sii", """
            SiiNunit
            {
            road_look : road.look0
            {
                name: "Road 1 lane double"
                lanes_left[]: traffic_lane.road.local
                lanes_right[]: traffic_lane.road.local
            }
            }
            """);
        Write("def/world/traffic_lane.sii", """
            SiiNunit
            {
            traffic_lane_data : traffic_lane.road.local
            {
                speed_class: local_road
                rank: 50
            }
            }
            """);
        var res = new DefinitionResolver(_overlay);
        var rl = res.GetRoadLook("look0");    // 无前缀引用（sector 存储格式）
        Assert.NotNull(rl);
        Assert.Equal("Road 1 lane double", rl!.DisplayName);
        Assert.Equal(2, rl.LaneCount);
        Assert.Equal("local_road", res.GetTrafficLane("traffic_lane.road.local")!.SpeedClass);
    }

    [Fact]
    public void Country_SpeedLimits_ParallelArrays()
    {
        Write("def/country/germany/speed_limits.sii", """
            SiiNunit
            {
            country_speed_limit : .speed_limit.car {
                vehicle_speed_class: car
                lane_speed_class[]: local_road
                limit[]: 100
                urban_limit[]: 50
                lane_speed_class[]: motorway
                limit[]: 0
                urban_limit[]: 0
            }
            }
            """);
        var res = new DefinitionResolver(_overlay);
        var de = res.GetCountry("germany");
        Assert.NotNull(de);
        Assert.Equal(100, de!.SpeedLimits["car"]["local_road"].Limit);
        Assert.Equal(50, de.SpeedLimits["car"]["local_road"].UrbanLimit);
        Assert.Equal(0, de.SpeedLimits["car"]["motorway"].Limit);
    }

    [Fact]
    public void Include_Recursion_BareSui()
    {
        Write("def/city.sii", """
            SiiNunit
            {
            @include "city/berlin.sui"
            }
            """);
        Write("def/city/berlin.sui", """
            city_data : city.berlin
            {
                city_name_localized: "@@berlin@@"
                country: germany
                population: 3650000
            }
            """);
        var res = new DefinitionResolver(_overlay);
        var b = res.GetCity("berlin");
        Assert.NotNull(b);
        Assert.Equal("germany", b!.Country);
        Assert.Equal(3650000, b.Population);
    }

    [Fact]
    public void InlineBrace_UnitHeader()
    {
        // header 与 { 同行（country_speed_limit 风格）
        Write("def/world/road_look.sii", """
            SiiNunit
            {
            road_look : road.inline {
                name: "inline brace"
            }
            }
            """);
        var res = new DefinitionResolver(_overlay);
        Assert.Equal("inline brace", res.GetRoadLook("road.inline")!.DisplayName);
    }

    [Fact]
    public void Company_And_TrafficRule_Overtake()
    {
        Write("def/company/acc.sui", """
            company_permanent: company.permanent.acc
            {
                name: "ACC"
                sort_name: "acc"
            }
            """);
        Write("def/world/traffic_lane.sii", """
            SiiNunit
            {
            traffic_lane_data : traffic_lane.road.local.overtake
            {
                speed_class: local_road
                traffic_rules[]: traffic_rule.overtake_alw
            }
            traffic_lane_data : traffic_lane.road.local
            {
                speed_class: local_road
                traffic_rules[]: traffic_rule.road
            }
            }
            """);
        var res = new DefinitionResolver(_overlay);
        Assert.Equal("ACC", res.GetCompany("acc")!.DisplayName);
        Assert.True(res.GetTrafficLane("traffic_lane.road.local.overtake")!.AllowsOvertake);
        Assert.False(res.GetTrafficLane("traffic_lane.road.local")!.AllowsOvertake);
    }
}
