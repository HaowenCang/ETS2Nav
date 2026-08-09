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

    /// <summary>按 item UID 解析坐标（首节点），供验证器填 X/Z（P1 §42）。</summary>
    public (double X, double Z) ItemCoordinate(ulong uid)
    {
        foreach (var sec in Sectors)
        {
            foreach (var item in sec.Items)
            {
                if (item.Uid != uid) continue;
                ulong nodeUid = item switch
                {
                    RoadItem r => r.Node0,
                    PrefabItem p when p.NodeUids.Length > 0 => p.NodeUids[0],
                    CityItem c => c.NodeUid,
                    CompanyItem m => m.MainNodeUid,
                    _ => 0,
                };
                if (nodeUid != 0)
                    foreach (var sec2 in Sectors)
                        foreach (var n in sec2.Nodes)
                            if (n.Uid == nodeUid) return (n.X, n.Z);
                break;
            }
        }
        return (0, 0);
    }
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
            // 按严重度稳定排序
            issues.AddRange(sink.OrderByDescending(i => (int)i.Severity));
        }
        // 坐标补填（P1-01 评审 M5）：验证器未填 X/Z 时按 SourceUid 推导
        for (int idx = 0; idx < issues.Count; idx++)
        {
            var i = issues[idx];
            if (i.SourceUid != 0 && i.X == 0 && i.Z == 0)
            {
                var (x, z) = ctx.ItemCoordinate(i.SourceUid);
                if (x != 0 || z != 0)
                    issues[idx] = i with { X = x, Z = z };
            }
        }
        return DiagnosticsReport.Build(issues);
    }
}
