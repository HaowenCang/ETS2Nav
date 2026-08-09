// ScsResource（P1 §9–11）：统一虚拟 SCS 文件系统。
// IScsResourceProvider（Exists/Open/Enumerate）+ HashFsProvider + DirectoryProvider
// + OverlayProvider（按优先级覆盖）+ DLC 检测 + Dataset fingerprint。
// GPL-3.0 — ETS2Nav 项目

using ScsHashFs;

namespace ScsResource;

/// <summary>统一资源接口（P1 §9）。虚拟路径格式：/def/world/...、/map/europe/...。</summary>
public interface IScsResourceProvider
{
    bool Exists(string virtualPath);
    Stream Open(string virtualPath);
    IEnumerable<string> Enumerate(string virtualDirectory);
}

/// <summary>HashFS archive 提供者（base.scs/def.scs/map.scs/DLC *.scs）。</summary>
public sealed class HashFsProvider : IScsResourceProvider, IDisposable
{
    private readonly HashFsReader _reader;

    public string ArchiveName { get; }

    public HashFsProvider(string archivePath)
    {
        ArchiveName = Path.GetFileName(archivePath);
        _reader = HashFsReader.Open(archivePath);
    }

    public bool Exists(string virtualPath)
    {
        if (!virtualPath.StartsWith('/')) virtualPath = "/" + virtualPath;
        return _reader.TryGetEntry(virtualPath) != null;
    }

    public Stream Open(string virtualPath)
    {
        if (!virtualPath.StartsWith('/')) virtualPath = "/" + virtualPath;
        var entry = _reader.TryGetEntry(virtualPath)
            ?? throw new FileNotFoundException($"'{virtualPath}' 不在 {ArchiveName} 中");
        return new MemoryStream(_reader.Extract(entry));
    }

    public IEnumerable<string> Enumerate(string virtualDirectory)
    {
        if (!virtualDirectory.StartsWith('/')) virtualDirectory = "/" + virtualDirectory;
        return _reader.EnumerateFiles(virtualDirectory);
    }

    public void Dispose() => _reader.Dispose();
}

/// <summary>目录提供者（解包后的 loose files / 测试 fixture）。</summary>
public sealed class DirectoryProvider : IScsResourceProvider
{
    private readonly string _root;

    public DirectoryProvider(string rootDir) => _root = Path.GetFullPath(rootDir);

    private string ToPhysical(string virtualPath)
    {
        var rel = virtualPath.TrimStart('/').Replace('/', Path.DirectorySeparatorChar);
        return Path.Combine(_root, rel);
    }

    public bool Exists(string virtualPath) => File.Exists(ToPhysical(virtualPath));

    public Stream Open(string virtualPath) => File.OpenRead(ToPhysical(virtualPath));

    public IEnumerable<string> Enumerate(string virtualDirectory)
    {
        var phys = ToPhysical(virtualDirectory);
        if (!Directory.Exists(phys)) yield break;
        foreach (var f in Directory.EnumerateFiles(phys, "*", SearchOption.AllDirectories))
            yield return "/" + Path.GetRelativePath(_root, f).Replace(Path.DirectorySeparatorChar, '/');
    }
}

/// <summary>覆盖提供者（P1 §10）：多个 provider 按优先级从低到高叠加，高层覆盖低层。</summary>
public sealed class OverlayProvider : IScsResourceProvider
{
    private readonly List<IScsResourceProvider> _providers;   // 索引 0 = 最低优先级

    public OverlayProvider(params IScsResourceProvider[] providers)
        => _providers = providers.ToList();

    public OverlayProvider Add(IScsResourceProvider provider)
    {
        _providers.Add(provider);
        return this;
    }

    public bool Exists(string virtualPath)
    {
        for (int i = _providers.Count - 1; i >= 0; i--)
            if (_providers[i].Exists(virtualPath)) return true;
        return false;
    }

    public Stream Open(string virtualPath)
    {
        // 从高优先级向下找第一个存在者
        for (int i = _providers.Count - 1; i >= 0; i--)
        {
            var p = _providers[i];
            if (p.Exists(virtualPath))
                return p.Open(virtualPath);
        }
        throw new FileNotFoundException($"资源不存在：{virtualPath}");
    }

    public IEnumerable<string> Enumerate(string virtualDirectory)
    {
        var seen = new HashSet<string>();
        foreach (var p in _providers)
            foreach (var path in p.Enumerate(virtualDirectory))
                if (seen.Add(path)) yield return path;
    }

    /// <summary>解析记录：某虚拟路径实际由哪个 provider 提供（诊断用，P1 §10）。</summary>
    public string? ResolveSource(string virtualPath)
    {
        for (int i = _providers.Count - 1; i >= 0; i--)
        {
            var p = _providers[i];
            if (p.Exists(virtualPath))
                return p switch
                {
                    HashFsProvider h => h.ArchiveName,
                    DirectoryProvider d => d.ToString() ?? "dir",
                    _ => p.GetType().Name,
                };
        }
        return null;
    }
}
