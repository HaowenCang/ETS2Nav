// Validation 基础设施（P1 §33–42）。
// Severity 分级：Fatal（数据集不可安全使用）/ Error（明确导航错误）/
// Warning（可疑但未证明）/ Info（统计与辅助诊断）。
// 每条 issue 含 code/severity/来源 UID/sector/坐标/描述，供 map-inspector 定位。
// GPL-3.0 — ETS2Nav 项目

namespace ScsValidation;

public enum ValidationSeverity
{
    Fatal = 0,
    Error = 1,
    Warning = 2,
    Info = 3,
}

public sealed record ValidationIssue
{
    public required string Code { get; init; }          // 如 "STRUCT_SELF_LOOP"
    public required ValidationSeverity Severity { get; init; }
    public required string Description { get; init; }

    /// <summary>关联的源 item UID（0 表示无）。</summary>
    public ulong SourceUid { get; init; }

    /// <summary>来源 sector（如 "sec+0002+0003"，null 表示无）。</summary>
    public string? Sector { get; init; }

    /// <summary>世界坐标（X, Z；Y 可选），用于 map-inspector 定位。</summary>
    public double X { get; init; }
    public double Z { get; init; }

    public override string ToString() =>
        $"[{Severity}] {Code} {(SourceUid != 0 ? SourceUid.ToString("x16") + " " : "")}" +
        $"{(Sector != null ? Sector + " " : "")}({X:F1},{Z:F1}) {Description}";
}
