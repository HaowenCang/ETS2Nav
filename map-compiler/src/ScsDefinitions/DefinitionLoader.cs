using ScsResource;
using ScsSii;

namespace ScsDefinitions;

/// <summary>
/// SII/SUI 加载器：经 IScsResourceProvider 读取，递归展开 @include（相对路径，.sii/.sui），
/// 合并为单一 SiiDocument，防循环。corpus 驱动的实际语法（P1 计划 §13）。
/// </summary>
public static class DefinitionLoader
{
    /// <summary>加载虚拟路径并递归展开 @include。失败抛 SiiParseException / 资源异常。</summary>
    public static SiiDocument Load(IScsResourceProvider provider, string virtualPath)
    {
        var doc = new SiiDocument();
        var visited = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        LoadInto(provider, Normalize(virtualPath), doc, visited);
        return doc;
    }

    private static void LoadInto(IScsResourceProvider provider, string vp, SiiDocument doc, HashSet<string> visited)
    {
        vp = Normalize(vp);
        if (!visited.Add(vp)) return;               // 防循环/重复 include
        if (!provider.Exists(vp))
            throw new FileNotFoundException($"definition 资源不存在：{vp}");
        string text;
        using (var s = provider.Open(vp))
        using (var r = new StreamReader(s))
            text = r.ReadToEnd();
        var parsed = SiiParser.Parse(text);
        // 先递归 include（相对当前文件目录；绝对路径按虚拟根解析），再合并自身 units（覆盖语义：自身优先）
        var dir = vp[..(vp.LastIndexOf('/') + 1)];
        foreach (var inc in parsed.Includes)
            LoadInto(provider, inc.StartsWith('/') ? inc : dir + inc, doc, visited);
        foreach (var u in parsed.Units) doc.Units.Add(u);
    }

    private static string Normalize(string vp)
    {
        vp = vp.Trim();
        if (!vp.StartsWith('/')) vp = "/" + vp;
        // 去除 ./ 与 ../ 段（简单规范化）
        var segs = vp.Split('/', StringSplitOptions.RemoveEmptyEntries).ToList();
        var outSegs = new List<string>();
        foreach (var seg in segs)
        {
            if (seg == ".") continue;
            if (seg == "..")
            {
                // 越界钳制改显式报错（P1-03 评审 m9）：虚拟根之上无目录可退
                if (outSegs.Count == 0) throw new ArgumentException($"非法虚拟路径（越界）：{vp}");
                outSegs.RemoveAt(outSegs.Count - 1);
                continue;
            }
            outSegs.Add(seg);
        }
        return "/" + string.Join('/', outSegs);
    }
}
