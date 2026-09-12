using ScsSector;
using ScsTests;

namespace ScsSector.Tests;

// 纯二进制工具函数，无外部输入依赖——portable 分类（P4R Batch 5 §17）。
public class ScsBinaryTests
{
    [Theory]
    [InlineData(0x0000000000750461UL, "euro2")]   // 实测 def.scs game_id
    [InlineData(0UL, "")]
    [InlineData(1UL, "0")]
    public void TokenDecode_RoundTrips(ulong value, string expected)
    {
        Assert.Equal(expected, ScsBinary.DecodeToken(value));
    }

    [Fact]
    public void TokenDecode_LongNames()
    {
        // "2ph"：'2'=idx3, 'p'=idx26, 'h'=idx18 → 3 + 26*38 + 18*38² = 3+988+25992 = 26983
        Assert.Equal("2ph", ScsBinary.DecodeToken(26983));
    }
}

// 集成测试：需要官方 scs_extractor 解包产物（ETS2NAV_EXTRACTED）。
//
// P4R Batch 5 §17：本类全部测试归入 GameAssetsRequired 分类。此前以
// `if (!File.Exists(BerlinSector)) return;` 静默通过；同一仓库的 ScsGraph.Tests
// 在同等缺失条件下却是 FAIL——同一前置条件两种相反结论，读者无从判断
// `dotnet test` 的 PASS 覆盖了什么。现在缺失即抛前置条件异常。
//
// 路径来源：环境变量 ETS2NAV_EXTRACTED；不再硬编码开发者本机路径（P4R Batch 4）
[Trait("Category", TestPaths.GameAssetsCategory)]
public class SectorFileTests
{
    [Fact]
    public void ReadBerlinSector_HeaderAndCounts()
    {
        var f = SectorFile.Read(
            TestPaths.RequireExtractedFile("base_map", "map", "europe", "sec-0002-0003.base"));
        Assert.Equal(907u, f.CoreMapVersion);
        Assert.Equal("euro2", f.GameId);
        Assert.Equal(3u, f.GameMapVersion);
        Assert.True(f.Items.Count > 400);
        Assert.True(f.Nodes.Count > 500);
    }

    [Fact]
    public void ReadBerlinSector_KnownNodePosition()
    {
        var f = SectorFile.Read(
            TestPaths.RequireExtractedFile("base_map", "map", "europe", "sec-0002-0003.base"));
        // 与 TruckLib 对照：uid 27d6a3fc3554000d pos=(-6786.65,35.15,-11983.65)
        var n = f.Nodes.FirstOrDefault(x => x.Uid == 0x27d6a3fc3554000dUL);
        Assert.NotNull(n);
        Assert.InRange(n!.X, -6787, -6786);
        Assert.InRange(n.Z, -11984, -11983);
    }

    [Fact]
    public void ReadBerlinSector_HasRoadsPrefabsAndCity()
    {
        var f = SectorFile.Read(
            TestPaths.RequireExtractedFile("base_map", "map", "europe", "sec-0002-0003.base"));
        Assert.True(f.Roads.Count() > 100);
        Assert.True(f.Prefabs.Count() > 30);
        var city = f.Items.OfType<CityItem>().FirstOrDefault();
        Assert.NotNull(city);
        // 数据实证：sec-0002-0003 的中心城市为 Osnabrück（柏林市区在其相邻 sector）
        Assert.Equal("osnabruck", city!.City);
    }

    [Fact]
    public void ReadAllSectors_NoExceptions()
    {
        var baseMapDir = TestPaths.RequireExtractedDirectory("base_map", "map", "europe");
        int items = 0, nodes = 0;
        foreach (var secFile in Directory.GetFiles(baseMapDir, "*.base"))
        {
            var f = SectorFile.Read(secFile);
            items += f.Items.Count;
            nodes += f.Nodes.Count;
        }
        // 与 TruckLib 全图对照的数量级
        Assert.True(items > 150000);
        Assert.True(nodes > 200000);
    }
}
