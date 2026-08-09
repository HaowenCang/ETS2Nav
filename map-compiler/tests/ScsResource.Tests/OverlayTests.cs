// ScsResource 单元测试（DirectoryProvider/OverlayProvider/GameInstall）。
// GPL-3.0 — ETS2Nav 项目

using ScsResource;
using Xunit;

namespace ScsResource.Tests;

public class OverlayTests : IDisposable
{
    private readonly string _lowDir, _highDir;
    private readonly DirectoryProvider _low, _high;

    public OverlayTests()
    {
        _lowDir = Path.Combine(Path.GetTempPath(), "ets2nav-res-low-" + Guid.NewGuid().ToString("N"));
        _highDir = Path.Combine(Path.GetTempPath(), "ets2nav-res-high-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_lowDir);
        Directory.CreateDirectory(_highDir);
        _low = new DirectoryProvider(_lowDir);
        _high = new DirectoryProvider(_highDir);
    }

    public void Dispose()
    {
        Directory.Delete(_lowDir, true);
        Directory.Delete(_highDir, true);
    }

    [Fact]
    public void Overlay_HighPriorityWins()
    {
        var d = Path.Combine(_lowDir, "def", "world");
        Directory.CreateDirectory(d);
        File.WriteAllText(Path.Combine(d, "x.sii"), "low");
        var d2 = Path.Combine(_highDir, "def", "world");
        Directory.CreateDirectory(d2);
        File.WriteAllText(Path.Combine(d2, "x.sii"), "high");

        var overlay = new OverlayProvider(_low, _high);
        using var r = new StreamReader(overlay.Open("/def/world/x.sii"));
        Assert.Equal("high", r.ReadToEnd());
        Assert.Equal(1, overlay.Enumerate("/def/world").Count());
    }

    [Fact]
    public void Overlay_MissingPath_Throws()
    {
        var overlay = new OverlayProvider(_low, _high);
        Assert.False(overlay.Exists("/no/such.sii"));
        Assert.Throws<FileNotFoundException>(() => overlay.Open("/no/such.sii"));
        Assert.Null(overlay.ResolveSource("/no/such.sii"));
    }

    [Fact]
    public void DirectoryProvider_Enumerate_Recursive()
    {
        var d = Path.Combine(_lowDir, "map", "europe");
        Directory.CreateDirectory(d);
        File.WriteAllText(Path.Combine(d, "sec+0000+0000.base"), "x");
        var files = _low.Enumerate("/map/europe").ToList();
        Assert.Contains("/map/europe/sec+0000+0000.base", files);
    }

    [Fact]
    public void Fingerprint_Changes_WhenArchiveChanges()
    {
        var dir = Path.Combine(Path.GetTempPath(), "ets2nav-game-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        try
        {
            var a = Path.Combine(dir, "base.scs");
            File.WriteAllText(a, "v1");
            File.SetLastWriteTimeUtc(a, DateTime.UtcNow);
            var inst1 = GameInstall.Detect(dir);
            var fp1 = inst1.ContentFingerprint;

            Thread.Sleep(1100);
            File.WriteAllText(a, "v2");   // 大小变化
            var inst2 = GameInstall.Detect(dir);
            Assert.NotEqual(fp1, inst2.ContentFingerprint);
            Assert.Single(inst2.Archives);
        }
        finally { Directory.Delete(dir, true); }
    }

    [Fact]
    public void Fingerprint_Changes_WhenDlcAdded()
    {
        var dir = Path.Combine(Path.GetTempPath(), "ets2nav-game-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(dir);
        try
        {
            File.WriteAllText(Path.Combine(dir, "base.scs"), "b");
            var fp1 = GameInstall.Detect(dir).ContentFingerprint;
            File.WriteAllText(Path.Combine(dir, "dlc_iberia.scs"), "d");
            var fp2 = GameInstall.Detect(dir).ContentFingerprint;
            Assert.NotEqual(fp1, fp2);
            Assert.Contains("iberia", GameInstall.Detect(dir).EnabledDlc);
        }
        finally { Directory.Delete(dir, true); }
    }

    [Fact]
    public void ResolveSource_ReturnsProvider()
    {
        var d1 = Path.Combine(Path.GetTempPath(), "ets2nav-rs-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(Path.Combine(d1, "def"));
        File.WriteAllText(Path.Combine(d1, "def", "x.sii"), "x");
        try
        {
            var overlay = new OverlayProvider(new DirectoryProvider(d1));
            Assert.NotNull(overlay.ResolveSource("/def/x.sii"));
            Assert.Null(overlay.ResolveSource("/def/y.sii"));
        }
        finally { Directory.Delete(d1, true); }
    }

    [Fact]
    public void Overlay_ThreeLayers_PriorityOrder()
    {
        var d1 = Path.Combine(Path.GetTempPath(), "ets2nav-3l1-" + Guid.NewGuid().ToString("N"));
        var d2 = Path.Combine(Path.GetTempPath(), "ets2nav-3l2-" + Guid.NewGuid().ToString("N"));
        var d3 = Path.Combine(Path.GetTempPath(), "ets2nav-3l3-" + Guid.NewGuid().ToString("N"));
        foreach (var d in new[] { d1, d2, d3 })
            Directory.CreateDirectory(Path.Combine(d, "def"));
        File.WriteAllText(Path.Combine(d1, "def", "x.sii"), "low");
        File.WriteAllText(Path.Combine(d3, "def", "x.sii"), "high");
        try
        {
            var overlay = new OverlayProvider(new DirectoryProvider(d1), new DirectoryProvider(d2), new DirectoryProvider(d3));
            using var r = new StreamReader(overlay.Open("/def/x.sii"));
            Assert.Equal("high", r.ReadToEnd());
        }
        finally
        {
            foreach (var d in new[] { d1, d2, d3 }) Directory.Delete(d, true);
        }
    }

    [Fact]
    public void DirectoryProvider_RejectsPathTraversal()
    {
        var d = Path.Combine(Path.GetTempPath(), "ets2nav-pt-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(d);
        try
        {
            var p = new DirectoryProvider(d);
            Assert.Throws<ArgumentException>(() => p.Open("/../../etc/passwd"));
        }
        finally { Directory.Delete(d, true); }
    }
}
