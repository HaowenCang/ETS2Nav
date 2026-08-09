// DiagnosticsReport：验证结果汇总 + JSON 导出（P1 §42 diagnostics.json 结构）。
// GPL-3.0 — ETS2Nav 项目

using System.Text.Json;

namespace ScsValidation;

public sealed class DiagnosticsReport
{
    public IReadOnlyList<ValidationIssue> Issues { get; }
    public int FatalCount { get; }
    public int ErrorCount { get; }
    public int WarningCount { get; }
    public int InfoCount { get; }

    public Dictionary<string, int> Stats { get; } = new();

    private DiagnosticsReport(IReadOnlyList<ValidationIssue> issues)
    {
        Issues = issues;
        FatalCount = issues.Count(i => i.Severity == ValidationSeverity.Fatal);
        ErrorCount = issues.Count(i => i.Severity == ValidationSeverity.Error);
        WarningCount = issues.Count(i => i.Severity == ValidationSeverity.Warning);
        InfoCount = issues.Count(i => i.Severity == ValidationSeverity.Info);
    }

    public static DiagnosticsReport Build(IReadOnlyList<ValidationIssue> issues) => new(issues);

    /// <summary>序列化为 diagnostics.json 兼容结构（P1 §42）。</summary>
    public string ToJson(bool includeIssues = true)
    {
        var root = new Dictionary<string, object>
        {
            ["fatal"] = FatalCount,
            ["errors"] = ErrorCount,
            ["warnings"] = WarningCount,
            ["infos"] = InfoCount,
            ["stats"] = Stats,
        };
        if (includeIssues)
        {
            root["issues"] = Issues.Select(i => new Dictionary<string, object?>
            {
                ["code"] = i.Code,
                ["severity"] = i.Severity.ToString().ToLowerInvariant(),
                ["description"] = i.Description,
                ["sourceUid"] = i.SourceUid == 0 ? null : i.SourceUid.ToString("x16"),
                ["sector"] = i.Sector,
                ["x"] = i.X,
                ["z"] = i.Z,
            });
        }
        return JsonSerializer.Serialize(root, new JsonSerializerOptions { WriteIndented = true });
    }

    public override string ToString() =>
        $"fatal={FatalCount} errors={ErrorCount} warnings={WarningCount} infos={InfoCount}";
}
