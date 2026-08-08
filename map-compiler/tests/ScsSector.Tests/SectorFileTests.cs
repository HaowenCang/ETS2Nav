using ScsSector;

namespace ScsSector.Tests;

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

// 集成测试：真实游戏 sector（文件缺失时跳过）
public class SectorFileTests
{
    private const string BerlinSector = @"E:\Projects\Pi\ETS2Nav\vendor\extracted\base_map\map\europe\sec-0002-0003.base";
    private const string BaseMapDir = @"E:\Projects\Pi\ETS2Nav\vendor\extracted\base_map\map\europe";

    [Fact]
    public void ReadBerlinSector_HeaderAndCounts()
    {
        if (!File.Exists(BerlinSector)) return;
        var f = SectorFile.Read(BerlinSector);
        Assert.Equal(907u, f.CoreMapVersion);
        Assert.Equal("euro2", f.GameId);
        Assert.Equal(3u, f.GameMapVersion);
        Assert.True(f.Items.Count > 400);
        Assert.True(f.Nodes.Count > 500);
    }

    [Fact]
    public void ReadBerlinSector_KnownNodePosition()
    {
        if (!File.Exists(BerlinSector)) return;
        var f = SectorFile.Read(BerlinSector);
        // 与 TruckLib 对照：uid 27d6a3fc3554000d pos=(-6786.65,35.15,-11983.65)
        var n = f.Nodes.FirstOrDefault(x => x.Uid == 0x27d6a3fc3554000dUL);
        Assert.NotNull(n);
        Assert.InRange(n!.X, -6787, -6786);
        Assert.InRange(n.Z, -11984, -11983);
    }

    [Fact]
    public void ReadBerlinSector_HasRoadsPrefabsAndCity()
    {
        if (!File.Exists(BerlinSector)) return;
        var f = SectorFile.Read(BerlinSector);
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
        if (!Directory.Exists(BaseMapDir)) return;
        int items = 0, nodes = 0;
        foreach (var secFile in Directory.GetFiles(BaseMapDir, "*.base"))
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
