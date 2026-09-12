using ScsHashFs;
using ScsTests;

namespace ScsHashFs.Tests;

// 集成测试使用真实游戏归档（未配置 ETS2_INSTALL 时跳过）。
// 路径来源：环境变量 ETS2_INSTALL / ETS2NAV_EXTRACTED；不再硬编码开发者本机路径（P4R Batch 4）。
public class HashFsReaderTests
{
    private static readonly string DefScs = TestPaths.GameFile("def.scs");

    [Fact]
    public void OpenDefScs_ReadsV2Header()
    {
        if (!File.Exists(DefScs)) return;
        using var r = HashFsReader.Open(DefScs);
        Assert.True(r.EntryCount > 60000, $"entry 数异常：{r.EntryCount}");
        Assert.Equal(0, r.Salt);
    }

    [Fact]
    public void ExtractSemaphoreProfile_MatchesKnownContent()
    {
        if (!File.Exists(DefScs)) return;
        using var r = HashFsReader.Open(DefScs);
        var entry = r.TryGetEntry("/def/world/semaphore_profile.sii");
        Assert.NotNull(entry);
        Assert.Equal(EntryType.File, entry!.Type);
        var text = r.ExtractText("/def/world/semaphore_profile.sii");
        Assert.Contains("SiiNunit", text);
        Assert.Contains("tr_sem_prof.2ph", text);
        Assert.Contains("interval[]", text);
    }

    [Fact]
    public void ListRootDirectory_ContainsDefAndMap()
    {
        if (!File.Exists(DefScs)) return;
        using var r = HashFsReader.Open(DefScs);
        var (subdirs, _) = r.ListDirectory("/");
        Assert.Contains("def", subdirs);
        Assert.Contains("world", r.ListDirectory("/def").Subdirs);
    }

    [Fact]
    public void EnumerateFiles_FindsWorldProfiles()
    {
        if (!File.Exists(DefScs)) return;
        using var r = HashFsReader.Open(DefScs);
        var files = r.EnumerateFiles("/def/world").ToList();
        Assert.Contains("/def/world/semaphore_profile.sii", files);
        Assert.Contains("/def/world/semaphore_model.sii", files);
    }

    [Fact]
    public void ExtractMatchAgainstOfficialExtractor()
    {
        // 对照：官方 scs_extractor 解包产物必须与直接读取一致
        var extracted = TestPaths.ExtractedFile("def", "def", "world", "semaphore_profile.sii");
        if (!File.Exists(DefScs) || !File.Exists(extracted)) return;
        using var r = HashFsReader.Open(DefScs);
        var text = r.ExtractText("/def/world/semaphore_profile.sii");
        var official = File.ReadAllText(extracted).Replace("\r\n", "\n").Trim();
        Assert.Equal(official, text.Trim());
    }
}
