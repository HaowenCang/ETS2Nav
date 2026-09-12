using System;
using System.IO;

namespace ScsTests;

/// <summary>
/// 测试输入路径解析（P4R Batch 4）。
///
/// 若干集成测试需要外部输入：ETS2 游戏安装目录，以及官方 scs_extractor 的解包产物。
/// 原实现把一名开发者的本机绝对路径写成 <c>const</c>，有两个后果：
/// 一是换机器或换检出位置后，测试读到的可能是**另一个工作区**里的数据；
/// 二是干净检出上的 PASS 可能实际来自原工作区，回归结果因此不可复现
/// （Batch 4 §17 要求 clean clone 不得读取原工作区内容）。
///
/// 现在按「环境变量 &gt; 仓库根相对路径」解析，仓库根由测试程序集位置向上查找，
/// 与仓库被放在哪个盘、哪一级目录无关。
/// </summary>
internal static class TestPaths
{
    /// <summary>仓库根：从测试程序集目录向上找到含 map-compiler/MapCompiler.sln 的目录。</summary>
    public static string RepoRoot { get; } = FindRepoRoot();

    /// <summary>游戏安装根目录（环境变量 ETS2_INSTALL）；未设置时为 null。</summary>
    public static string? GameInstall { get; } = NonEmpty(Environment.GetEnvironmentVariable("ETS2_INSTALL"));

    /// <summary>
    /// scs_extractor 解包根（含 base_map/ 与 def/）：环境变量 ETS2NAV_EXTRACTED，
    /// 否则 &lt;repoRoot&gt;/vendor/extracted。
    /// </summary>
    public static string ExtractedRoot { get; } =
        NonEmpty(Environment.GetEnvironmentVariable("ETS2NAV_EXTRACTED"))
        ?? Path.Combine(RepoRoot, "vendor", "extracted");

    /// <summary>游戏安装内的文件路径；未配置 ETS2_INSTALL 时返回不存在的路径，交由调用点的存在性检查处理。</summary>
    public static string GameFile(string relative)
        => GameInstall is null ? string.Empty : Path.Combine(GameInstall, relative);

    /// <summary>解包产物内的文件路径。</summary>
    public static string ExtractedFile(params string[] relative)
    {
        var parts = new string[relative.Length + 1];
        parts[0] = ExtractedRoot;
        Array.Copy(relative, 0, parts, 1, relative.Length);
        return Path.Combine(parts);
    }

    private static string? NonEmpty(string? v) => string.IsNullOrWhiteSpace(v) ? null : v;

    private static string FindRepoRoot()
    {
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        while (dir is not null)
        {
            if (File.Exists(Path.Combine(dir.FullName, "map-compiler", "MapCompiler.sln")))
                return dir.FullName;
            dir = dir.Parent;
        }
        throw new InvalidOperationException(
            "无法从测试程序集目录 " + AppContext.BaseDirectory +
            " 向上定位仓库根（判据：存在 map-compiler/MapCompiler.sln）。");
    }
}
