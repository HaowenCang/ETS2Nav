// ValidationEngine：注册并运行验证器，汇总 issues 与统计。
// 结构（P1 §34）：ValidationEngine ─┬─ StructuralValidator
//                                   ├─ ReferenceValidator
//                                   ├─ DirectionValidator
//                                   ├─ ConnectivityValidator
//                                   └─ GeometryValidator
// （Junction/Semantic/Poi/Dataset 验证器在 P1-04 之后补充）
// GPL-3.0 — ETS2Nav 项目

using ScsGraph;
using ScsSector;
using ScsValidation.Validators;

namespace ScsValidation;

public interface IGraphValidator
{
    string Name { get; }
    void Validate(ValidationContext ctx, List<ValidationIssue> sink);
}

public sealed class ValidationContext
{
    public required IReadOnlyList<SectorFile> Sectors { get; init; }
    public required RoadGraph Graph { get; init; }
}

public sealed class ValidationEngine
{
    private readonly List<IGraphValidator> _validators = new();

    public ValidationEngine Register(IGraphValidator validator)
    {
        _validators.Add(validator);
        return this;
    }

    public DiagnosticsReport Run(ValidationContext ctx)
    {
        var issues = new List<ValidationIssue>();
        foreach (var v in _validators)
        {
            var sink = new List<ValidationIssue>();
            v.Validate(ctx, sink);
            // 按严重度稳定排序（Info 在前不影响 Fatal 统计）
            issues.AddRange(sink.OrderByDescending(i => (int)i.Severity));
        }
        return DiagnosticsReport.Build(issues);
    }
}
