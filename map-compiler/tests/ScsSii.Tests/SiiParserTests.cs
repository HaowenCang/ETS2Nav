using ScsSii;

namespace ScsSii.Tests;

public class SiiParserTests
{
    private static SiiDocument Parse(string s) => SiiParser.Parse(s);

    [Fact]
    public void ParsesBasicUnit()
    {
        var doc = Parse("""
            SiiNunit
            {
            tr_semaphore_profile : tr_sem_prof.2ph
            {
            	name: "two phases"
            	interval[]: (15.0, 2.0, 23.0, 2.0)
            	cycle[]: 0.0
            	cycle[]: 21.0
            }
            }
            """);
        Assert.Single(doc.Units);
        var u = doc.Units[0];
        Assert.Equal("tr_semaphore_profile", u.Class);
        Assert.Equal("tr_sem_prof.2ph", u.Name);
        var interval = u.Values("interval[]").Single();
        Assert.Equal(SiiValueKind.Tuple, interval.Kind);
        Assert.Equal(new[] { 15.0, 2.0, 23.0, 2.0 }, interval.Tuple);
        Assert.Equal(2, u.Values("cycle[]").Count());
        Assert.Equal(0.0, u.Values("cycle[]").First().Num);
        Assert.Equal(21.0, u.Values("cycle[]").Last().Num);
    }

    [Fact]
    public void HandlesCommentsAndTrailingComments()
    {
        var doc = Parse("""
            SiiNunit
            {
            # 顶层注释
            tr_semaphore_profile : tr_sem_prof.2ph		# 行尾注释
            {
            	name: "a#b"		# 字符串内 # 不截断
            	cycle[]: 3.5
            }
            }
            """);
        var u = doc.Units.Single();
        Assert.Equal("a#b", u.Values("name").Single().Str);
        Assert.Equal(3.5, u.Values("cycle[]").Single().Num);
    }

    [Fact]
    public void ParsesIncludeAndLocalUnits()
    {
        var doc = Parse("""
            SiiNunit
            {
            @include "city/berlin.sui"
            license_plate_data : .berlin.lp.car
            {
            	type: car
            	templates[]: "B033 1222"
            }
            }
            """);
        Assert.Equal(new[] { "city/berlin.sui" }, doc.Includes);
        var u = doc.Units.Single();
        Assert.Equal(".berlin.lp.car", u.Name);
        Assert.Equal("car", u.Values("type").Single().Str);
    }

    [Fact]
    public void HandlesRealWorldProfileFields()
    {
        var doc = Parse("""
            SiiNunit
            {
            tr_semaphore_profile : tr_sem_prof.cr_1x1
            {
            	name: "crossroad 1x1"
            	model[0]: "single"
            	type[0]: traffic_light_minor
            	type[1]: traffic_light_major
            	interval[0]: (15.0, 2.0, 23.0, 2.0)
            	sleep_time_start: 1410
            	sleep_time_end: 180
            	inherited: tr_sem_prof.2ph
            }
            }
            """);
        var u = doc.Units.Single();
        Assert.Equal(2, u.Values("type[]").Count());
        Assert.Equal("traffic_light_minor", u.Values("type[]").First().Str);
        Assert.Equal(1410.0, u.Values("sleep_time_start").Single().Num);
        Assert.Equal("tr_sem_prof.2ph", u.Values("inherited").Single().Str);
    }

    [Fact]
    public void HandlesBoolAndNegativeNumbers()
    {
        var doc = Parse("""
            SiiNunit
            {
            gate : gate.x
            {
            	manual_control: false
            	offset: -3.25
            }
            }
            """);
        var u = doc.Units.Single();
        Assert.False(u.Values("manual_control").Single().Bool);
        Assert.Equal(-3.25, u.Values("offset").Single().Num);
    }

    [Fact]
    public void ThrowsOnMalformedHeader()
    {
        Assert.Throws<SiiParseException>(() => Parse("SiiNunit\n{\nfoo bar\n}\n"));
    }
}
