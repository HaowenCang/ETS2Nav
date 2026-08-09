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

    [Fact]
    public void StripsTrailingSlashSlashComment()   // M1：行尾 // 注释（corpus 98 文件/2225 行）
    {
        var doc = Parse("SiiNunit\n{\nroad_look : road.x\n{\n\tdistance_default: 9.0 // default value\n\tcorner0[]: \"rus_14001\" //sh\n}\n}\n");
        var u = doc.Units.Single();
        Assert.Equal(SiiValueKind.Number, u.Values("distance_default").Single().Kind);
        Assert.Equal(9.0, u.Values("distance_default").Single().Num);
        Assert.Equal("rus_14001", u.Values("corner0[]").Single().Str);
    }

    [Fact]
    public void ParsesMultilineString()   // M4：多行字符串（prefab.sii 等 5 文件实测）
    {
        var doc = Parse("SiiNunit\n{\nintro : intro.x\n{\n\ttext: \"line1\nline2\nline3\"\n\tnum: 5\n}\n}\n");
        var u = doc.Units.Single();
        Assert.Equal("line1\nline2\nline3", u.Values("text").Single().Str);
        Assert.Equal(5.0, u.Values("num").Single().Num);   // 多行字符串后属性继续正常解析
    }

    [Fact]
    public void ParsesTupleFloatSuffix()   // M3：元组元素 f 后缀（1,095 文件实测）
    {
        var doc = Parse("SiiNunit\n{\ndashboard : d.x\n{\n\tpos: (0.0f, 0.01132f, 0.000f)\n}\n}\n");
        var t = doc.Units.Single().Values("pos").Single().Tuple!;
        Assert.Equal(3, t.Count);
        Assert.Equal(0.01132, t[1], 5);
    }

    [Fact]
    public void EmptyFileReturnsEmptyDoc()   // M7：空文件/仅注释/裸 }（2 个真实空 .sui）
    {
        Assert.Empty(Parse("").Units);
        Assert.Empty(Parse("   \n\n").Units);
        Assert.Empty(Parse("# only comment\n// another\n").Units);
        Assert.Empty(Parse("}").Units);
    }

    [Fact]
    public void BareSuiStartingWithInclude()   // m2：裸 .sui 以 @include 开头（academy goal 系列）
    {
        var doc = Parse("@include \"goal_common.sui\"\nfinish_data : finish.x\n{\n\tflag: true\n}\n");
        Assert.Equal(new[] { "goal_common.sui" }, doc.Includes);
        Assert.Single(doc.Units);
    }

    [Fact]
    public void IncludeInsideUnitBody()   // M2：unit 体内 @include（paint_job 2,996 文件实测）
    {
        var doc = Parse("SiiNunit\n{\naccessory_paint_job_data : p.x\n{\n\t@include \"p_settings.sui\"\n\tname: \"x\"\n}\n}\n");
        Assert.Contains("p_settings.sui", doc.Includes);
        Assert.Equal("x", doc.Units.Single().Values("name").Single().Str);
    }

    [Fact]
    public void ParsesHexValue()   // m5：hex 值（vendor_id: 0x10DE）
    {
        var doc = Parse("SiiNunit\n{\ngfx : g.x\n{\n\tvendor_id: 0x10DE\n}\n}\n");
        Assert.Equal(0x10DE, (int)doc.Units.Single().Values("vendor_id").Single().Num);
    }

    [Fact]
    public void StripsHeaderTrailingComment()   // m1：header 行尾注释（trailer_double.sii 实测）
    {
        var doc = Parse("SiiNunit\n{\ntrailer : academy.trailer.double   # double 13,6m + 7,8m\n{\n\tname: \"x\"\n}\n}\n");
        Assert.Equal("academy.trailer.double", doc.Units.Single().Name);
    }

    [Fact]
    public void SkipsBlockComment()   // m3：/** 块注释（mail_data.sii 实测）
    {
        var doc = Parse("SiiNunit\n{\n/**\n * Test / debug messages\n */\nmail : m.x\n{\n\tflag: true\n}\n}\n");
        Assert.Single(doc.Units);
    }

    [Fact]
    public void StripsBom()   // m10：BOM（17 文件实测）
    {
        var doc = Parse("\uFEFFSiiNunit\n{\nroad_look : road.x\n{\n}\n}\n");
        Assert.Single(doc.Units);
    }

    [Fact]
    public void IncludeWithTrailingComment()   // m6：@include 行尾注释
    {
        var doc = Parse("SiiNunit\n{\n@include \"city/berlin.sui\" # comment\n}\n");
        Assert.Equal(new[] { "city/berlin.sui" }, doc.Includes);
    }
}
