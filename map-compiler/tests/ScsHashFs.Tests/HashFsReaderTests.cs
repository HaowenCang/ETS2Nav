using ScsHashFs;

namespace ScsHashFs.Tests;

// 集成测试使用真实游戏归档（跳过当文件不存在）。
// 参考路径：E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2\
public class HashFsReaderTests
{
    private const string GameDir = @"E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2";
    private static readonly string DefScs = Path.Combine(GameDir, "def.scs");

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
        var extracted = @"E:\Projects\Pi\ETS2Nav\vendor\extracted\def\def\world\semaphore_profile.sii";
        if (!File.Exists(DefScs) || !File.Exists(extracted)) return;
        using var r = HashFsReader.Open(DefScs);
        var text = r.ExtractText("/def/world/semaphore_profile.sii");
        var official = File.ReadAllText(extracted).Replace("\r\n", "\n").Trim();
        Assert.Equal(official, text.Trim());
    }
}
