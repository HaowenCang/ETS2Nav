// SCS SII 文本格式解析器（独立实现，基于官方 def 文件观察与社区格式描述）
// GPL-3.0 — ETS2Nav 项目
// 2026-08 corpus 驱动增强（P1-03 评审修复）：行尾 // 注释、多行字符串、块内 @include、
// 元组 f 后缀、hex 值、空文件、裸 .sui 以 @include 开头、/** 块注释、header 尾注释、BOM

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
    public List<string> Includes { get; } = new();      // @include 路径（顶层与块内）
}

public static class SiiParser
{
    /// <summary>解析 SII 文本。失败抛 SiiParseException 并带行号。
    /// 支持三种形态：标准 SiiNunit 包装、裸 unit 序列（.sui include 片段）、空文件。</summary>
    public static SiiDocument Parse(string text)
    {
        var doc = new SiiDocument();
        var lines = text.TrimStart('\uFEFF').Replace("\r\n", "\n").Split('\n');
        int i = 0;
        var (topClass, topName) = PeekHeader(lines, ref i);
        if (topClass is null || topClass == "}")
            return doc;                                  // 空文件 / 仅注释 / 裸 '}'
        if (topClass == "@include")
        {
            doc.Includes.Add(topName!);                  // 裸 .sui 以 @include 开头（academy goal 系列）
            i++;
        }
        else if (topClass == "SiiNunit")
        {
            i++; // 消费顶层 header 行
            SkipToOpenBrace(lines, ref i);
        }
        else
        {
            // 裸 unit 文件：当前 header 即为第一个 unit
            ParseUnit(lines, ref i, topClass, topName!, doc);
        }
        ParseTopLevel(lines, ref i, doc);
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
            if (cls == "@include")
            {
                doc.Includes.Add(name!);
                i++;
                continue;
            }
            i++; // 消费 header 行
            ParseUnit(lines, ref i, cls, name!, doc);
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
            ParseAttributes(lines, ref i, unit, doc);
            doc.Units.Add(unit);
        }
        else
        {
            // 无块体（单行 unit，值引用形式）——忽略体，仅记录
            doc.Units.Add(new SiiUnit { Class = cls, Name = name });
        }
    }

    private static void ParseAttributes(string[] lines, ref int i, SiiUnit unit, SiiDocument doc)
    {
        while (i < lines.Length)
        {
            var (key, value, consumed) = ParseAttribute(lines, ref i);
            if (key == "}") { i++; return; }
            if (key == "@include")
            {
                // 块内 @include（paint_job 等 2996 文件实测）：属性片段合并语义未建模，记录路径供展开
                doc.Includes.Add(value!.Str!);
                i++;
                continue;
            }
            if (key is null) { i++; continue; }   // 空行/注释
            unit.Attributes.Add((key, value!));
            i += consumed;   // 多行字符串可能跨行（consumed > 1）
        }
    }

    /// <summary>解析一行属性。返回 null（空行/注释）、("}", null) 块结束、("@include", path)。</summary>
    private static (string? Key, SiiValue? Value, int Consumed) ParseAttribute(string[] lines, ref int i)
    {
        string line = lines[i];
        if (line.Trim() == "}") return ("}", null, 1);
        var (key, rest) = SplitKey(line);
        if (key is null) return (null, null, 1);
        if (key == "@include") return ("@include", SiiValue.OfString(rest), 1);
        string valueText = StripComment(rest).Trim();
        if (valueText.Length == 0) throw new SiiParseException($"属性 {key} 缺值（行 {i + 1}）");
        // 多行字符串：值以 " 开头但未闭合 → 跨行拼接（prefab.sii/intro_data.sii 等实测）
        if (valueText.StartsWith('"') && !IsStringClosed(valueText))
        {
            int consumed = 1;
            while (i + consumed < lines.Length)
            {
                valueText += "\n" + lines[i + consumed].TrimEnd('\r');
                consumed++;
                if (IsStringClosed(valueText)) break;
            }
            if (!IsStringClosed(valueText)) throw new SiiParseException($"字符串未闭合（行 {i + 1}）");
            return (key, SiiValue.OfString(valueText[1..^1]), consumed);
        }
        return (key, ParseValue(valueText, i + 1), 1);
    }

    private static (string?, string) SplitKey(string line)
    {
        var t = line.TrimStart();
        if (t.Length == 0 || t[0] == '#' || t.StartsWith("//") || t.StartsWith("/**")) return (null, "");
        if (t.StartsWith("@include")) return ("@include", ExtractIncludePath(t));
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

    /// <summary>字符串外剥离行尾注释（# 与 //）。字符串内保留；转义引号不切换字符串态。</summary>
    private static string StripComment(string s)
    {
        var sb = new System.Text.StringBuilder(s.Length);
        bool inStr = false;
        for (int k = 0; k < s.Length; k++)
        {
            char c = s[k];
            if (c == '"' && (k == 0 || s[k - 1] != '\\')) { inStr = !inStr; sb.Append(c); }
            else if (!inStr && c == '#') break;
            else if (!inStr && c == '/' && k + 1 < s.Length && s[k + 1] == '/') break;
            else sb.Append(c);
        }
        return sb.ToString();
    }

    /// <summary>引号配对（转义引号除外）。偶数 = 已闭合。</summary>
    private static bool IsStringClosed(string s)
    {
        int quotes = 0;
        for (int k = 0; k < s.Length; k++)
            if (s[k] == '"' && (k == 0 || s[k - 1] != '\\')) quotes++;
        return quotes % 2 == 0;
    }

    private static string ExtractIncludePath(string t)
    {
        var q = StripComment(t["@include".Length..]).Trim();
        if (q.Length >= 2 && q.StartsWith('"') && q.EndsWith('"')) q = q[1..^1];
        return q;
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
            // 分隔符支持逗号与分号（corpus 实测：部分文件用 (1.000000; 0.000000) 分号形式）
            var parts = v[1..^1].Split(new[] { ',', ';' }, StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries);
            var nums = new double[parts.Length];
            for (int k = 0; k < parts.Length; k++)
                nums[k] = ParseNumber(parts[k], lineNo, "元组元素");
            return SiiValue.OfTuple(nums);
        }
        if (v == "true") return SiiValue.OfBool(true);
        if (v == "false") return SiiValue.OfBool(false);
        // 数字优先于 token（避免 "21.0" 因含小数点被误判为引用）
        try { return SiiValue.OfNumber(ParseNumber(v, lineNo)); }
        catch (SiiParseException) { }
        // 其余视为 token/枚举/标识符
        return SiiValue.OfToken(v);
    }

    /// <summary>SCS 数值：支持 f/F 后缀（0.0f）、0x 十六进制、小数、科学计数法。</summary>
    private static double ParseNumber(string v, int lineNo, string what = "数字")
    {
        var t = v.Trim();
        if (t.EndsWith('f') || t.EndsWith('F')) t = t[..^1];
        if (t.StartsWith("0x", StringComparison.OrdinalIgnoreCase))
        {
            if (ulong.TryParse(t[2..], System.Globalization.NumberStyles.HexNumber, null, out var h))
                return h;
            throw new SiiParseException($"{what}非十六进制：{v}（行 {lineNo}）");
        }
        if (double.TryParse(t, System.Globalization.CultureInfo.InvariantCulture, out var d)) return d;
        throw new SiiParseException($"{what}非数字：{v}（行 {lineNo}）");
    }

    // ---- 行迭代辅助 ----

    private static (string? Class, string? Name) PeekHeader(string[] lines, ref int i)
    {
        while (i < lines.Length)
        {
            var t = lines[i].Trim();
            if (t.Length == 0 || t.StartsWith('#') || t.StartsWith("//")) { i++; continue; }
            if (t.StartsWith("/**"))
            {
                // C 风格块注释：跳至 */ 所在行（mail_data.sii 实测）
                while (i < lines.Length && !lines[i].Contains("*/")) i++;
                if (i < lines.Length) i++;
                continue;
            }
            if (t == "}") return ("}", null);
            if (t.StartsWith("@include")) return ("@include", ExtractIncludePath(t));
            return SplitHeader(t);
        }
        return (null, null);
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
        var name = StripComment(t[(colon + 1)..]).Trim();   // header 行尾注释剥离（m1）
        return (t[..colon].Trim(), name);
    }

    private static void SkipToOpenBrace(string[] lines, ref int i)
    {
        while (i < lines.Length)
        {
            var t = lines[i].Trim();
            if (t == "{") { i++; return; }
            if (t.Length > 0 && !t.StartsWith('#') && !t.StartsWith("//") && !t.StartsWith("/**"))
                throw new SiiParseException($"期望 '{{'（行 {i + 1}）");
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
            if (t.Length > 0 && !t.StartsWith('#') && !t.StartsWith("//") && !t.StartsWith("/**"))
            {
                // header 之后的非 '{' 行：unit 无块体（如 `foo : .bar` 单行定义）——不消费该行
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
