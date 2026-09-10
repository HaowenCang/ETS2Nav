// ScsResource（P6 §9 覆盖缺口修复，2026-08-12）：mod 变更检测。
//
// 背景：原 content_fingerprint 只覆盖游戏安装目录（版本 + archive 元数据 + DLC 集合），
// 而 mod 安装不改变安装目录任何元数据——数据集构建后启用/更新 mod 会使地图数据改变
// 但指纹仍报 MATCH（审计 a4-perf-truth M5 登记的**真实漏报路径**）。本文件补齐该覆盖。
//
// 设计要点：
//   1. 扫描两类 mod 来源：本地 mod 目录（Documents/ETS2/mod）与 Steam Workshop 内容目录
//      （steamapps/workshop/content/227300）。
//   2. **地图相关性探测**：仅有改写 /map 的 mod 才会使数据集失效（涂装/内饰类不会）。
//      探测方式为读取 archive 条目表并检查 /map 前缀——不读文件内容，成本与条目数成正比。
//   3. **内容哈希分层**：默认仅元数据（名/大小/UTC mtime），与安装指纹口径一致且成本恒定；
//      deep=true 时对**地图相关** mod 追加 SHA-256 内容哈希。分层原因：实测本机 mod 目录
//      11.2 GB（ProMods 全量），无条件内容哈希会让每次构建付出分钟级 I/O。
//   4. 指纹串含 mode 标记，避免「浅指纹」与「深指纹」误判为数据变更。
//
// GPL-3.0 — ETS2Nav 项目

using System.Security.Cryptography;
using System.Text;

namespace ScsResource;

/// <summary>单个 mod archive。</summary>
/// <param name="Name">文件名（含扩展名）。</param>
/// <param name="Path">物理路径。</param>
/// <param name="Size">字节数。</param>
/// <param name="Modified">最后写入时间（UTC 参与指纹）。</param>
/// <param name="MapRelevant">是否含 /map 条目（true=会使数据集失效；false=已探测确认无；null=无法探测）。</param>
/// <param name="ContentHash">deep 模式下地图相关 mod 的 SHA-256（其余为 null）。</param>
/// <param name="Source">来源：local（mod 目录）或 workshop。</param>
public sealed record ModArchive(
    string Name,
    string Path,
    long Size,
    DateTime Modified,
    bool? MapRelevant,
    string? ContentHash,
    string Source);

/// <summary>mod 扫描结果。</summary>
public sealed record ModScanResult(
    IReadOnlyList<ModArchive> Mods,
    string Fingerprint,
    int MapRelevant,
    int Unprobeable,
    bool Deep,
    IReadOnlyList<string> Roots);

/// <summary>mod 来源扫描与指纹（P6 §9 覆盖缺口修复）。</summary>
public static class ModScanner
{
    /// <summary>ETS2 的 Steam AppID（Workshop 内容目录名）。</summary>
    private const string Ets2AppId = "227300";

    /// <summary>推断 Documents 下的 ETS2 用户目录。</summary>
    public static string DefaultDocumentsDir()
    {
        var docs = Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments);
        return Path.Combine(docs, "Euro Truck Simulator 2");
    }

    /// <summary>从游戏安装目录推断 Steam Workshop 内容目录（&lt;library&gt;/steamapps/workshop/content/227300）。</summary>
    public static string? InferWorkshopDir(string gameDir)
    {
        // gameDir 形如 <library>/steamapps/common/Euro Truck Simulator 2
        var common = Path.GetDirectoryName(gameDir.TrimEnd('\\', '/'));
        if (common == null) return null;
        var steamapps = Path.GetDirectoryName(common);
        if (steamapps == null) return null;
        var ws = Path.Combine(steamapps, "workshop", "content", Ets2AppId);
        return Directory.Exists(ws) ? ws : null;
    }

    /// <summary>枚举并（可选）哈希 mod 来源，返回结果与指纹。</summary>
    /// <param name="gameDir">游戏安装目录（用于推断 Workshop 目录；可为 null）。</param>
    /// <param name="documentsDir">ETS2 用户目录（含 mod/）；null 用默认推断。</param>
    /// <param name="deep">true = 对地图相关 mod 追加内容哈希。</param>
    public static ModScanResult Scan(string? gameDir, string? documentsDir = null, bool deep = false)
    {
        var roots = new List<string>();
        var mods = new List<ModArchive>();

        var docs = documentsDir ?? DefaultDocumentsDir();
        var modDir = Path.Combine(docs, "mod");
        if (Directory.Exists(modDir))
        {
            roots.Add(modDir);
            // 顶层为标准布局；递归可覆盖玩家手工分层的极少情况
            foreach (var f in Directory.EnumerateFiles(modDir, "*", SearchOption.AllDirectories))
                if (IsModArchive(f))
                    mods.Add(Describe(f, "local", deep));
        }

        if (gameDir != null)
        {
            var ws = InferWorkshopDir(gameDir);
            if (ws != null)
            {
                roots.Add(ws);
                // Workshop 布局：<content>/<publishedFileId>/<file>.scs
                foreach (var f in Directory.EnumerateFiles(ws, "*", SearchOption.AllDirectories))
                    if (IsModArchive(f))
                        mods.Add(Describe(f, "workshop", deep));
            }
        }

        mods.Sort((a, b) => string.CompareOrdinal(a.Path, b.Path));
        var fp = ComputeFingerprint(mods, deep);
        return new ModScanResult(
            mods,
            fp,
            mods.Count(m => m.MapRelevant == true),
            mods.Count(m => m.MapRelevant == null),
            deep,
            roots);
    }

    private static bool IsModArchive(string path)
    {
        var ext = Path.GetExtension(path);
        return ext.Equals(".scs", StringComparison.OrdinalIgnoreCase)
            || ext.Equals(".zip", StringComparison.OrdinalIgnoreCase);
    }

    private static ModArchive Describe(string path, string source, bool deep)
    {
        var fi = new FileInfo(path);
        var (mapRelevant, _) = ProbeMapRelevance(path);
        string? contentHash = null;
        if (deep && mapRelevant == true)
        {
            try
            {
                using var fs = File.OpenRead(path);
                contentHash = Convert.ToHexString(SHA256.HashData(fs)).ToLowerInvariant();
            }
            catch
            {
                contentHash = null;   // 读取失败：降级为元数据（指纹仍含 size/mtime）
            }
        }
        return new ModArchive(Path.GetFileName(path), path, fi.Length, fi.LastWriteTime, mapRelevant, contentHash, source);
    }

    /// <summary>探测 archive 是否含 /map 条目。true=含；false=已成功读取且不含；null=读取失败（格式不支持/损坏）。</summary>
    private static (bool? MapRelevant, string? Error) ProbeMapRelevance(string path)
    {
        // HashFS（.scs magic 头）
        try
        {
            using var reader = ScsHashFs.HashFsReader.Open(path);
            foreach (var p in reader.EnumerateFiles("/map"))
                return (true, null);          // 命中即返回（不枚举全表）
            return (false, null);
        }
        catch (Exception exHashFs)
        {
            // zipfs（标准 ZIP 容器，可为 .scs 或 .zip）——实测 ProMods 地图 mod 即此格式，
            // 仅靠 HashFS 探测会漏判，故补 ZIP 中央目录枚举（ScsResource.ZipfsProbe）
            if (ZipfsProbe.LooksLikeZip(path))
            {
                try
                {
                    return (ZipfsProbe.ContainsPrefix(path, "/map"), null);
                }
                catch (Exception exZip)
                {
                    return (null, $"{exHashFs.GetType().Name}/{exZip.GetType().Name}");
                }
            }
            // 两种格式都不识别：如实标为不可探测，由 diagnostics 计数暴露，不静默当 false
            return (null, exHashFs.GetType().Name);
        }
    }

    /// <summary>指纹 = 每条 mod 的「来源:名:大小:mtimeUTC ticks:地图相关性[:内容哈希]」+ 模式标记 → SHA-256。
    /// 模式标记使浅/深指纹不同源，避免把「换算法」误报为「数据变更」。</summary>
    public static string ComputeFingerprint(IReadOnlyList<ModArchive> mods, bool deep)
    {
        var sb = new StringBuilder();
        sb.Append("mods-v1|mode=").Append(deep ? "deep" : "meta");
        foreach (var m in mods)
        {
            sb.Append('|').Append(m.Source).Append(':').Append(m.Name)
              .Append(':').Append(m.Size)
              .Append(':').Append(m.Modified.ToUniversalTime().Ticks)
              .Append(':').Append(m.MapRelevant switch { true => "map", false => "nomap", _ => "unknown" });
            if (deep) sb.Append(':').Append(m.ContentHash ?? "-");
        }
        return Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(sb.ToString()))).ToLowerInvariant();
    }

    /// <summary>地图相关 mod 名单（供 check 输出与报告）。</summary>
    public static IEnumerable<ModArchive> MapAltering(IReadOnlyList<ModArchive> mods)
        => mods.Where(m => m.MapRelevant == true);
}
