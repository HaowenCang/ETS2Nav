// SCS SII 文本格式解析器（独立实现，基于官方 def 文件观察与社区格式描述）
// GPL-3.0 — ETS2Nav 项目

namespace ScsSii;

public enum SiiValueKind { String, Number, Bool, Token, Tuple }

public sealed class SiiValue
{
    public SiiValueKind Kind { get; }
    public string? Str { get; }
    public double Num { get; }
    public bool Bool { get; }
    public IReadOnlyList<double>? Tuple { get; }

    private SiiValue(SiiValueKind kind, string? str = null, double num = 0, bool b = false, IReadOnlyList<double>? tuple = null)
    {
        Kind = kind; Str = str; Num = num; Bool = b; Tuple = tuple;
    }
    public static SiiValue OfString(string s) => new(SiiValueKind.String, str: s);
    public static SiiValue OfNumber(double d) => new(SiiValueKind.Number, num: d);
    public static SiiValue OfBool(bool b) => new(SiiValueKind.Bool, b: b);
    public static SiiValue OfToken(string t) => new(SiiValueKind.Token, str: t);
    public static SiiValue OfTuple(IReadOnlyList<double> t) => new(SiiValueKind.Tuple, tuple: t);

    public override string ToString() => Kind switch
    {
        SiiValueKind.String => $"\"{Str}\"",
        SiiValueKind.Number => Num.ToString("0.0########"),
        SiiValueKind.Bool => Bool ? "true" : "false",
        SiiValueKind.Token => Str!,
        SiiValueKind.Tuple => $"({string.Join(", ", Tuple!)})",
        _ => "?"
    };
}

/// <summary>SII unit：`class : name { attr... }` 或局部 `class : .name`。</summary>
public sealed class SiiUnit
{
    public required string Class { get; init; }
    public required string Name { get; init; }          // 原始名（含前导 . 表示局部）
    public List<(string Key, SiiValue Value)> Attributes { get; } = new();

    /// <summary>取某键全部值（数组键 `key[]` 会自然重复出现）。</summary>
    public IEnumerable<SiiValue> Values(string key) =>
        Attributes.Where(a => a.Key == key).Select(a => a.Value);
}

/// <summary>SII 文档：SiiNunit 顶层块内的全部 unit。</summary>
public sealed class SiiDocument
{
    public List<SiiUnit> Units { get; } = new();
    public List<string> Includes { get; } = new();      // @include 路径
}

public static class SiiParser
{
    /// <summary>解析 SII 文本。失败抛 SiiParseException 并带行号。
    /// 支持两种顶层：标准 SiiNunit 包装，以及裸 unit 序列（.sui include 片段，无 SiiNunit）。</summary>
    public static SiiDocument Parse(string text)
    {
        var doc = new SiiDocument();
        var lines = text.Replace("\r\n", "\n").Split('\n');
        int i = 0;
        var (topClass, topName) = NextUnitHeader(lines, ref i);
        if (topClass == "SiiNunit")
        {
            i++; // 消费顶层 header 行
            SkipToOpenBrace(lines, ref i);
            ParseTopLevel(lines, ref i, doc);
        }
        else if (topClass is null || topClass == "}")
        {
            // 空文件
        }
        else
        {
            // 裸 unit 文件：当前 header 即为第一个 unit（不含 SiiNunit）
            ParseUnit(lines, ref i, topClass, topName, doc);
            ParseTopLevel(lines, ref i, doc);
        }
        return doc;
    }

    private static void ParseTopLevel(string[] lines, ref int i, SiiDocument doc)
    {
        while (i < lines.Length)
        {
            var (cls, name) = PeekHeader(lines, ref i);
            if (cls is null) break;
            if (cls == "}")
            {
                i++; // 顶层闭括号
                break;
            }
            if (cls.StartsWith("@include"))
            {
                doc.Includes.Add(name);
                i++;
                continue;
            }
            i++; // 消费 header 行
            ParseUnit(lines, ref i, cls, name, doc);
        }
    }

    private static void ParseUnit(string[] lines, ref int i, string cls, string name, SiiDocument doc)
    {
        // `xxx : name {`（header 与块同行）与 `xxx : name\n{` 两种格式
        bool inlineBrace = name.EndsWith('{');
        if (inlineBrace) name = name[..^1].TrimEnd();
        if (inlineBrace || TrySkipToOpenBraceOrLineEnd(lines, ref i))
        {
            var unit = new SiiUnit { Class = cls, Name = name };
            ParseAttributes(lines, ref i, unit);
            doc.Units.Add(unit);
        }
        else
        {
            // 无块体（单行 unit，值引用形式）——忽略体，仅记录
            doc.Units.Add(new SiiUnit { Class = cls, Name = name });
        }
    }

    private static void ParseAttributes(string[] lines, ref int i, SiiUnit unit)
    {
        while (i < lines.Length)
        {
            var (key, value, consumed) = ParseAttribute(lines, ref i);
            if (key == "}") { i++; return; }
            if (key is null) { i++; continue; }   // 空行/注释
            unit.Attributes.Add((key, value));
            i++;
        }
    }

    /// <summary>解析一行属性。返回 null（空行/注释）或 ("}", null) 表示块结束。</summary>
    private static (string? Key, SiiValue? Value, int Consumed) ParseAttribute(string[] lines, ref int i)
    {
        string line = lines[i];
        if (line.Trim() == "}") return ("}", null, 1);
        var (key, rest) = SplitKey(line);
        if (key is null) return (null, null, 1);
        // 值可能跨行（罕见；tuple 通常单行）。此处仅支持单行值 + 行尾注释。
        string valueText = StripComment(rest).Trim();
        if (valueText.Length == 0) throw new SiiParseException($"属性 {key} 缺值（行 {i + 1}）");
        return (key, ParseValue(valueText, i + 1), 1);
    }

    private static (string?, string) SplitKey(string line)
    {
        var t = line.TrimStart();
        if (t.Length == 0 || t[0] == '#' || t.StartsWith("//")) return (null, "");
        int colon = t.IndexOf(':');
        if (colon < 0) throw new SiiParseException($"无法解析行：{line.Trim()}");
        var key = t[..colon].Trim();
        // 带索引数组键归一化：type[0] → type[]（语义为同一数组字段）
        int lb = key.LastIndexOf('[');
        if (lb > 0 && key.EndsWith(']'))
        {
            var idx = key[(lb + 1)..^1];
            if (idx.Length > 0 && idx.All(char.IsDigit)) key = key[..lb] + "[]";
        }
        return (key, t[(colon + 1)..]);
    }

    private static string StripComment(string s)
    {
        // 字符串外的 '#' 起注释。逐字符处理，保留字符串内 '#'
        var sb = new System.Text.StringBuilder(s.Length);
        bool inStr = false;
        foreach (char c in s)
        {
            if (c == '"') inStr = !inStr;
            else if (c == '#' && !inStr) break;
            sb.Append(c);
        }
        return sb.ToString();
    }

    private static SiiValue ParseValue(string v, int lineNo)
    {
        if (v.StartsWith('"'))
        {
            if (v.Length < 2 || v[^1] != '"') throw new SiiParseException($"字符串未闭合（行 {lineNo}）");
            return SiiValue.OfString(v[1..^1]);
        }
        if (v.StartsWith('(') && v.EndsWith(')'))
        {
            var parts = v[1..^1].Split(',', StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries);
            var nums = new double[parts.Length];
            for (int k = 0; k < parts.Length; k++)
                if (!double.TryParse(parts[k], System.Globalization.CultureInfo.InvariantCulture, out nums[k]))
                    throw new SiiParseException($"元组元素非数字：{parts[k]}（行 {lineNo}）");
            return SiiValue.OfTuple(nums);
        }
        if (v == "true") return SiiValue.OfBool(true);
        if (v == "false") return SiiValue.OfBool(false);
        // 数字优先于 token（避免 "21.0" 因含小数点被误判为引用）
        if (double.TryParse(v, System.Globalization.CultureInfo.InvariantCulture, out var d)) return SiiValue.OfNumber(d);
        // 其余视为 token/枚举/标识符
        return SiiValue.OfToken(v);
    }

    // ---- 行迭代辅助 ----

    private static (string? Class, string? Name) PeekHeader(string[] lines, ref int i)
    {
        while (i < lines.Length)
        {
            var t = lines[i].Trim();
            if (t.Length == 0 || t.StartsWith('#') || t.StartsWith("//")) { i++; continue; }
            if (t == "}") return ("}", null);
            if (t.StartsWith("@include"))
            {
                var q = t["@include".Length..].Trim();
                var path = q.StartsWith('"') && q.EndsWith('"') ? q[1..^1] : q;
                return ("@include", path);
            }
            return SplitHeader(t);
        }
        return (null, null);
    }

    private static (string Class, string Name) NextUnitHeader(string[] lines, ref int i)
    {
        var (c, n) = PeekHeader(lines, ref i);
        if (c is null || c == "}") throw new SiiParseException("未找到 unit header");
        return (c, n!);
    }

    private static (string Class, string Name) SplitHeader(string t)
    {
        int colon = t.IndexOf(':');
        if (colon < 0)
        {
            // 顶层 SiiNunit 无冒号合法；含空格的畸形行报错
            if (t.Contains(' ')) throw new SiiParseException($"unit header 缺 '：'：{t}");
            return (t, "");
        }
        return (t[..colon].Trim(), t[(colon + 1)..].Trim());
    }

    private static void SkipToOpenBrace(string[] lines, ref int i)
    {
        while (i < lines.Length)
        {
            var t = lines[i].Trim();
            if (t == "{") { i++; return; }
            if (t.Length > 0 && !t.StartsWith('#') && !t.StartsWith("//")) throw new SiiParseException($"期望 '{{'（行 {i + 1}）");
            i++;
        }
        throw new SiiParseException("文件在 '{{' 前结束");
    }

    /// <summary>跳过块体起始 '{'（返回 true）或整行（unit 无块体，返回 false）。</summary>
    private static bool TrySkipToOpenBraceOrLineEnd(string[] lines, ref int i)
    {
        while (i < lines.Length)
        {
            var t = lines[i].Trim();
            if (t == "{") { i++; return true; }
            if (t.Length > 0 && !t.StartsWith('#') && !t.StartsWith("//"))
            {
                // header 之后的非 '{' 行：unit 无块体（如 `foo : .bar` 单行定义）——不消费该行？
                // 该行就是 header 自身（无块），直接返回 false 且不推进（调用方已消费 header）。
                return false;
            }
            i++;
        }
        return false;
    }
}

public sealed class SiiParseException : Exception
{
    public SiiParseException(string message) : base(message) { }
}
