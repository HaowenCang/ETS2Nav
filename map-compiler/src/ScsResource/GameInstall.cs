// ScsResource（P1 §10–11）：ETS2 安装检测 + DLC 识别 + Overlay 构建 + Dataset fingerprint。
// GPL-3.0 — ETS2Nav 项目

using System.Diagnostics;
using System.Security.Cryptography;
using System.Text;

namespace ScsResource;

public sealed record GameArchive(string Name, string Path, long Size, DateTime Modified);

public sealed class GameInstall
{
    public required string GameDir { get; init; }
    public required IReadOnlyList<GameArchive> Archives { get; init; }
    public required IReadOnlyList<string> EnabledDlc { get; init; }
    public string? GameVersion { get; init; }
    public string ContentFingerprint { get; set; } = "";

    /// <summary>检测 ETS2 安装目录：识别 base/def/map 与官方 DLC archives。</summary>
    public static GameInstall Detect(string gameDir)
    {
        var archives = new List<GameArchive>();
        var enabledDlc = new List<string>();

        // ETS2 根目录下的 .scs（base.scs / def.scs / base_map.scs / base_aux.scs / dlc_*.scs）
        foreach (var f in Directory.EnumerateFiles(gameDir, "*.scs"))
        {
            var fi = new FileInfo(f);
            var name = Path.GetFileName(f);
            archives.Add(new GameArchive(name, f, fi.Length, fi.LastWriteTime));
            if (name.StartsWith("dlc_", StringComparison.OrdinalIgnoreCase))
                enabledDlc.Add(name[4..^4]);
        }
        // 部分 DLC archive 在 dlc/ 子目录
        var dlcDir = Path.Combine(gameDir, "dlc");
        if (Directory.Exists(dlcDir))
        {
            foreach (var f in Directory.EnumerateFiles(dlcDir, "*.scs", SearchOption.AllDirectories))
            {
                var fi = new FileInfo(f);
                var name = Path.GetFileName(f);
                archives.Add(new GameArchive(name, f, fi.Length, fi.LastWriteTime));
                if (name.StartsWith("dlc_", StringComparison.OrdinalIgnoreCase))
                    enabledDlc.Add(name[4..^4]);
            }
        }
        archives.Sort((a, b) => string.Compare(a.Name, b.Name, StringComparison.Ordinal));

        string? version = null;
        var exePath = Path.Combine(gameDir, "bin", "win_x64", "eurotrucks2.exe");
        if (File.Exists(exePath))
            version = FileVersionInfo.GetVersionInfo(exePath).FileVersion;

        var inst = new GameInstall
        {
            GameDir = gameDir,
            Archives = archives,
            EnabledDlc = enabledDlc.Distinct().OrderBy(x => x).ToList(),
            GameVersion = version,
        };
        inst.ContentFingerprint = inst.ComputeFingerprint();
        return inst;
    }

    /// <summary>contentFingerprint（P1 §11）：游戏版本 + archive 名/大小/UTC 时间 + DLC 集合 → SHA-256。</summary>
    public string ComputeFingerprint()
    {
        var sb = new StringBuilder();
        sb.Append(GameVersion ?? "?");
        foreach (var a in Archives)
            sb.Append('|').Append(a.Name).Append(':').Append(a.Size).Append(':').Append(a.Modified.ToUniversalTime().Ticks);
        sb.Append("|dlc=").Append(string.Join(",", EnabledDlc));
        var bytes = SHA256.HashData(Encoding.UTF8.GetBytes(sb.ToString()));
        return Convert.ToHexString(bytes).ToLowerInvariant();
    }

    /// <summary>构建覆盖顺序与 ETS2 一致的 OverlayProvider（base → DLC 优先级序，P1 §10）。
    /// 仅加载含 /map/ 目录的 archive（涂装/配件 DLC 不参与地图解析）。</summary>
    public OverlayProvider BuildOverlay()
    {
        var overlay = new OverlayProvider();
        var ordered = Archives.OrderBy(a => Rank(a.Name)).ToList();
        foreach (var a in ordered)
        {
            if (!a.Name.StartsWith("base_") && !a.Name.Equals("base.scs") && !a.Name.Equals("def.scs")
                && !a.Name.Equals("core.scs") && !a.Name.StartsWith("dlc_"))
                continue;   // 跳过无关 archive
            if (a.Name.StartsWith("dlc_"))
            {
                // 地图 DLC 探测：打开条目表看是否含 /map/ 目录（失败不静默——警告并跳过，P1-02 评审 M3）
                try
                {
                    using var probe = ScsHashFs.HashFsReader.Open(a.Path);
                    if (!probe.EnumerateFiles("/map").Any()) continue;
                }
                catch (Exception ex)
                {
                    Console.Error.WriteLine($"[ScsResource] 警告：dlc archive {a.Name} 打不开（{ex.GetType().Name}: {ex.Message}），已跳过");
                    continue;
                }
            }
            overlay.Add(new HashFsProvider(a.Path));
        }
        return overlay;
    }

    private static int Rank(string name) => name.ToLowerInvariant() switch
    {
        "base.scs" => 0,
        "def.scs" => 1,
        "base_map.scs" => 2,
        "base_aux.scs" => 3,
        "base_navi.scs" => 4,
        "core.scs" => 5,
        "base_cfg.scs" => 6,
        "base_share.scs" => 7,
        "base_vehicle.scs" => 8,
        _ => 100,   // dlc_* 按名序（ETS2 实际优先级含 dlc 依赖序，名序为近似）
    };
}
