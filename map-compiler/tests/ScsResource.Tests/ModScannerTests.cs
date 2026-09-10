// ModScanner / ZipfsProbe 单元测试（P6 §9 覆盖缺口修复）。
// 目的：锁定 mod 指纹的变更检测语义、地图相关性判定的两种容器格式，以及封闭性
//      （不触碰真实 mod 目录——测试必须在任意机器上快速且确定）。
// GPL-3.0 — ETS2Nav 项目

using System.IO.Compression;
using ScsResource;
using Xunit;

namespace ScsResource.Tests;

public class ModScannerTests : IDisposable
{
    private readonly string _root;      // 模拟 ETS2 用户目录（含 mod/）
    private readonly string _libRoot;   // 模拟 Steam 库根（含 steamapps/）
    private readonly string _gameDir;   // 模拟安装目录 <lib>/steamapps/common/Euro Truck Simulator 2

    public ModScannerTests()
    {
        var id = Guid.NewGuid().ToString("N");
        _root = Path.Combine(Path.GetTempPath(), "ets2nav-mods-" + id);
        _libRoot = Path.Combine(Path.GetTempPath(), "ets2nav-steamlib-" + id);
        _gameDir = Path.Combine(_libRoot, "steamapps", "common", "Euro Truck Simulator 2");
        Directory.CreateDirectory(Path.Combine(_root, "mod"));
        Directory.CreateDirectory(_gameDir);
    }

    public void Dispose()
    {
        Directory.Delete(_root, true);
        Directory.Delete(_libRoot, true);
    }

    private string ModDir => Path.Combine(_root, "mod");
    private string WorkshopDir => Path.Combine(_libRoot, "steamapps", "workshop", "content", "227300");

    private static void WriteZip(string path, params string[] entryNames)
    {
        using var fs = File.Create(path);
        using var za = new ZipArchive(fs, ZipArchiveMode.Create);
        foreach (var n in entryNames)
        {
            var e = za.CreateEntry(n);
            using var s = e.Open();
            s.WriteByte(0x41);
        }
    }

    /// <summary>无 mod 目录内容 → 空指纹，且重复扫描确定。</summary>
    [Fact]
    public void Scan_EmptyModDir_Deterministic()
    {
        var a = ModScanner.Scan(gameDir: null, documentsDir: _root);
        var b = ModScanner.Scan(gameDir: null, documentsDir: _root);
        Assert.Empty(a.Mods);
        Assert.Equal(a.Fingerprint, b.Fingerprint);
    }

    /// <summary>新增 mod → 指纹变化（这正是原实现漏报的路径：mod 不影响安装目录元数据）。</summary>
    [Fact]
    public void Scan_Changes_WhenModAdded()
    {
        var fp1 = ModScanner.Scan(gameDir: null, documentsDir: _root).Fingerprint;
        File.WriteAllText(Path.Combine(ModDir, "some-mod.scs"), "not-a-real-archive");
        var fp2 = ModScanner.Scan(gameDir: null, documentsDir: _root).Fingerprint;
        Assert.NotEqual(fp1, fp2);
    }

    /// <summary>mod 内容变化（大小改变）→ 指纹变化。</summary>
    [Fact]
    public void Scan_Changes_WhenModContentChanges()
    {
        var p = Path.Combine(ModDir, "m.scs");
        File.WriteAllText(p, "aaaa");
        var fp1 = ModScanner.Scan(gameDir: null, documentsDir: _root).Fingerprint;
        File.WriteAllText(p, "bbbbbbbb");
        var fp2 = ModScanner.Scan(gameDir: null, documentsDir: _root).Fingerprint;
        Assert.NotEqual(fp1, fp2);
    }

    /// <summary>zipfs（标准 ZIP）含 /map 条目 → 判为地图相关；不含 → 非地图相关。
    /// 该用例锁定 ProMods 场景（其地图 mod 为 zipfs，HashFS 读取器无法打开）。</summary>
    [Fact]
    public void Scan_DetectsMapRelevance_ForZipContainer()
    {
        WriteZip(Path.Combine(ModDir, "map-mod.scs"), "map/europe/sec+0000+0000.base", "def/world/x.sii");
        WriteZip(Path.Combine(ModDir, "paint-mod.zip"), "vehicle/truck/foo.pmd", "def/vehicle/y.sii");

        var scan = ModScanner.Scan(gameDir: null, documentsDir: _root);
        var mapMod = scan.Mods.Single(m => m.Name == "map-mod.scs");
        var paintMod = scan.Mods.Single(m => m.Name == "paint-mod.zip");
        Assert.True(mapMod.MapRelevant);
        Assert.False(paintMod.MapRelevant);
        Assert.Equal(1, scan.MapRelevant);
        Assert.Equal(0, scan.Unprobeable);
        Assert.Single(ModScanner.MapAltering(scan.Mods));
    }

    /// <summary>非 ZIP 非 HashFS 的垃圾文件 → 标为不可探测（null），而非静默当 false。</summary>
    [Fact]
    public void Scan_UnprobeableArchive_MarkedNull()
    {
        File.WriteAllText(Path.Combine(ModDir, "junk.scs"), "this is not an archive at all");
        var scan = ModScanner.Scan(gameDir: null, documentsDir: _root);
        Assert.Single(scan.Mods);
        Assert.Null(scan.Mods[0].MapRelevant);
        Assert.Equal(1, scan.Unprobeable);
        Assert.Equal(0, scan.MapRelevant);
    }

    /// <summary>deep 模式仅对地图相关 mod 计内容哈希；非地图相关保持 null。</summary>
    [Fact]
    public void Scan_DeepHashesMapRelevantOnly()
    {
        WriteZip(Path.Combine(ModDir, "map-mod.scs"), "map/europe/a.base");
        WriteZip(Path.Combine(ModDir, "paint-mod.scs"), "vehicle/a.pmd");
        var scan = ModScanner.Scan(gameDir: null, documentsDir: _root, deep: true);
        Assert.NotNull(scan.Mods.Single(m => m.Name == "map-mod.scs").ContentHash);
        Assert.Null(scan.Mods.Single(m => m.Name == "paint-mod.scs").ContentHash);
        Assert.True(scan.Deep);
    }

    /// <summary>浅/深指纹不同源（mode 参与哈希）——避免把「换算法」误报为「数据变更」。</summary>
    [Fact]
    public void Fingerprint_ModeIsPartOfHash()
    {
        WriteZip(Path.Combine(ModDir, "map-mod.scs"), "map/europe/a.base");
        var meta = ModScanner.Scan(gameDir: null, documentsDir: _root, deep: false);
        var deep = ModScanner.Scan(gameDir: null, documentsDir: _root, deep: true);
        Assert.NotEqual(meta.Fingerprint, deep.Fingerprint);
    }

    /// <summary>workshop 内容目录被纳入（<library>/steamapps/workshop/content/227300/&lt;id&gt;/x.scs）。</summary>
    [Fact]
    public void Scan_IncludesWorkshopContent()
    {
        var ws = Path.Combine(WorkshopDir, "123456");
        Directory.CreateDirectory(ws);
        WriteZip(Path.Combine(ws, "ws-mod.scs"), "map/europe/b.base");
        var scan = ModScanner.Scan(_gameDir, _root);
        Assert.Contains(scan.Mods, m => m.Source == "workshop" && m.Name == "ws-mod.scs");
        Assert.Equal(1, scan.MapRelevant);
    }

    /// <summary>workshop 目录不存在时不影响（workshop 缺席是常见情况）。</summary>
    [Fact]
    public void Scan_NoWorkshopDir_StillWorks()
    {
        var scan = ModScanner.Scan(_gameDir, _root);
        Assert.Empty(scan.Mods);
        Assert.DoesNotContain(scan.Roots, r => r.Contains("workshop"));
    }
}

public class ZipfsProbeTests : IDisposable
{
    private readonly string _dir;

    public ZipfsProbeTests()
    {
        _dir = Path.Combine(Path.GetTempPath(), "ets2nav-zip-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_dir);
    }

    public void Dispose() => Directory.Delete(_dir, true);

    private static void WriteZip(string path, params string[] names)
    {
        using var fs = File.Create(path);
        using var za = new ZipArchive(fs, ZipArchiveMode.Create);
        foreach (var n in names)
        {
            var e = za.CreateEntry(n);
            using var s = e.Open();
            s.WriteByte(0x41);
        }
    }

    [Fact]
    public void EnumerateEntryNames_ReturnsAll()
    {
        var p = Path.Combine(_dir, "a.scs");
        WriteZip(p, "def/a.sii", "map/europe/x.base", "vehicle/b.pmd");
        var names = ZipfsProbe.EnumerateEntryNames(p).ToList();
        Assert.Equal(3, names.Count);
        Assert.Contains("map/europe/x.base", names);
    }

    [Fact]
    public void ContainsPrefix_MatchesWithAndWithoutLeadingSlash()
    {
        var p = Path.Combine(_dir, "b.scs");
        WriteZip(p, "map/europe/x.base");
        Assert.True(ZipfsProbe.ContainsPrefix(p, "/map"));
        Assert.True(ZipfsProbe.ContainsPrefix(p, "map"));
        Assert.False(ZipfsProbe.ContainsPrefix(p, "/vehicle"));
    }

    [Fact]
    public void ContainsPrefix_NoMapEntries_ReturnsFalse()
    {
        var p = Path.Combine(_dir, "c.scs");
        WriteZip(p, "vehicle/a.pmd", "def/vehicle/b.sii");
        Assert.False(ZipfsProbe.ContainsPrefix(p, "/map"));
    }

    [Fact]
    public void LooksLikeZip_DistinguishesNonZip()
    {
        var z = Path.Combine(_dir, "z.scs");
        WriteZip(z, "a");
        Assert.True(ZipfsProbe.LooksLikeZip(z));

        var junk = Path.Combine(_dir, "j.scs");
        File.WriteAllText(junk, "ScsHashFs-magic-not-really");
        Assert.False(ZipfsProbe.LooksLikeZip(junk));
    }

    [Fact]
    public void EnumerateEntryNames_NonZip_Throws()
    {
        var junk = Path.Combine(_dir, "k.scs");
        File.WriteAllText(junk, new string('x', 100));
        Assert.ThrowsAny<Exception>(() => ZipfsProbe.EnumerateEntryNames(junk).ToList());
    }

    /// <summary>空 ZIP（仅 EOCD）→ 枚举为空，不抛异常。</summary>
    [Fact]
    public void EnumerateEntryNames_EmptyZip_ReturnsEmpty()
    {
        var p = Path.Combine(_dir, "empty.zip");
        using (var fs = File.Create(p))
        using (new ZipArchive(fs, ZipArchiveMode.Create)) { }
        Assert.Empty(ZipfsProbe.EnumerateEntryNames(p));
    }
}
