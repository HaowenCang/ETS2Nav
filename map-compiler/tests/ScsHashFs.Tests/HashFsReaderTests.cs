using ScsHashFs;
using ScsTests;

namespace ScsHashFs.Tests;

// 集成测试：需要正版 ETS2 游戏归档（ETS2_INSTALL）。
//
// P4R Batch 5 §17：本类全部测试归入 GameAssetsRequired 分类。此前每个测试都以
// `if (!File.Exists(DefScs)) return;` 开头——输入缺失时它们"以零工作量通过"，
// 于是 `dotnet test` 在干净检出上给出 5 passed，读者无法分辨这 5 个测试究竟
// 验证了归档解析，还是什么都没做。现在输入缺失即抛前置条件异常（FAIL），
// 需要排除时用 Trait/--filter 显式排除，排除的事实写在命令行里。
//
// 路径来源：环境变量 ETS2_INSTALL / ETS2NAV_EXTRACTED；不再硬编码开发者本机路径（P4R Batch 4）。
[Trait("Category", TestPaths.GameAssetsCategory)]
public class HashFsReaderTests
{
    [Fact]
    public void OpenDefScs_ReadsV2Header()
    {
        using var r = HashFsReader.Open(TestPaths.RequireGameFile("def.scs"));
        Assert.True(r.EntryCount > 60000, $"entry 数异常：{r.EntryCount}");
        Assert.Equal(0, r.Salt);
    }

    [Fact]
    public void ExtractSemaphoreProfile_MatchesKnownContent()
    {
        using var r = HashFsReader.Open(TestPaths.RequireGameFile("def.scs"));
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
        using var r = HashFsReader.Open(TestPaths.RequireGameFile("def.scs"));
        var (subdirs, _) = r.ListDirectory("/");
        Assert.Contains("def", subdirs);
        Assert.Contains("world", r.ListDirectory("/def").Subdirs);
    }

    [Fact]
    public void EnumerateFiles_FindsWorldProfiles()
    {
        using var r = HashFsReader.Open(TestPaths.RequireGameFile("def.scs"));
        var files = r.EnumerateFiles("/def/world").ToList();
        Assert.Contains("/def/world/semaphore_profile.sii", files);
        Assert.Contains("/def/world/semaphore_model.sii", files);
    }

    [Fact]
    public void ExtractMatchAgainstOfficialExtractor()
    {
        // 对照：官方 scs_extractor 解包产物必须与直接读取一致
        var defScs = TestPaths.RequireGameFile("def.scs");
        var extracted = TestPaths.RequireExtractedFile("def", "def", "world", "semaphore_profile.sii");
        using var r = HashFsReader.Open(defScs);
        var text = r.ExtractText("/def/world/semaphore_profile.sii");
        var official = File.ReadAllText(extracted).Replace("\r\n", "\n").Trim();
        Assert.Equal(official, text.Trim());
    }
}
