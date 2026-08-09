using ScsResource;
using ScsDefinitions;
using ScsSii;

namespace ScsPrefab;

/// <summary>
/// Prefab 解析器：prefab token（sector PrefabItem.Model，无前缀如 mod_ger_67）→
/// /def/world/prefab*.sii 的 prefab_model unit（prefab. 前缀）→ prefab_desc 路径 → .ppd 加载。
/// </summary>
public sealed class PrefabResolver
{
    private readonly IScsResourceProvider _provider;
    private readonly Dictionary<string, string> _descByToken = new();   // "mod_ger_67" → "/prefab2/xxx.ppd"
    private readonly Dictionary<string, PrefabDescriptor> _cache = new();

    public IReadOnlyDictionary<string, string> DescByToken => _descByToken;

    /// <summary>加载失败的 PPD（token → 错误信息）。</summary>
    public IReadOnlyList<(string Token, string Error)> FailedPpds => _failed;
    private readonly List<(string Token, string Error)> _failed = new();

    public PrefabResolver(IScsResourceProvider provider)
    {
        _provider = provider;
        LoadPrefabIndex();
    }

    private void LoadPrefabIndex()
    {
        foreach (var f in _provider.Enumerate("/def/world").Where(p => p.EndsWith(".sii") && p.Contains("prefab.")))
        {
            SiiDocument doc;
            try { doc = DefinitionLoader.Load(_provider, f); }
            catch { continue; }
            foreach (var u in doc.Units)
            {
                if (u.Class != "prefab_model") continue;
                // unit 名 prefab.mod_ger_67 → 无前缀键 mod_ger_67（sector 引用形式）
                var bare = u.Name.StartsWith("prefab.") ? u.Name["prefab.".Length..] : u.Name;
                var desc = u.Attributes.FirstOrDefault(a => a.Key == "prefab_desc").Value?.Str;
                if (desc is null || desc.Length == 0) continue;
                _descByToken[bare] = desc;
            }
        }
    }

    /// <summary>按 token 加载 prefab 描述（缓存）。失败返回 null 并记录 FailedPpds。</summary>
    public PrefabDescriptor? Load(string bareToken)
    {
        if (_cache.TryGetValue(bareToken, out var hit)) return hit;
        if (!_descByToken.TryGetValue(bareToken, out var ppdPath))
        {
            _failed.Add((bareToken, "prefab.sii 无此 token 的 prefab_desc"));
            return null;
        }
        try
        {
            if (!_provider.Exists(ppdPath))
            {
                _failed.Add((bareToken, $"PPD 不存在：{ppdPath}"));
                return null;
            }
            using var s = _provider.Open(ppdPath);
            var pd = PpdReader.Read(s, ppdPath);
            _cache[bareToken] = pd;
            return pd;
        }
        catch (Exception ex)
        {
            _failed.Add((bareToken, $"{ex.GetType().Name}: {ex.Message}"));
            return null;
        }
    }

    public static string ReadToken(ulong value)
    {
        if (value == 0) return "";
        int length = 1;
        while (Pow38(length) - 1 < value) length++;
        var chars = new char[length];
        for (int i = length; i > 0; i--)
        {
            ulong pow = Pow38(i - 1);
            chars[length - i] = "\000123456789abcdefghijklmnopqrstuvwxyz_"[(int)(value / pow)];
            value %= pow;
        }
        return new string(chars);
    }

    private static ulong Pow38(int n)
    {
        ulong r = 1;
        for (int i = 0; i < n; i++) r *= 38;
        return r;
    }
}
