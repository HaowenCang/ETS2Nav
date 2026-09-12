using ScsSector;
using ScsGraph;
using ScsTests;

namespace ScsGraph.Tests;

public class GraphValidatorTests
{
    // 解包根不再硬编码开发者本机路径（P4R Batch 4）：见 TestPaths
    private static readonly string BerlinDir =
        TestPaths.ExtractedFile("base_map", "map", "europe");

    private static SectorFile ReadSector(string name)
    {
        var p = Path.Combine(BerlinDir, name + ".base");
        if (!File.Exists(p)) throw new FileNotFoundException(p);
        return SectorFile.Read(p);
    }

    private static readonly string[] BerlinRegion =
    [
        "sec+0001-0001", "sec+0001-0002", "sec+0001-0003", "sec+0001-0004",
        "sec+0002-0001", "sec+0002-0002", "sec+0002-0003", "sec+0002-0004",
        "sec+0003-0001", "sec+0003-0002", "sec+0003-0003", "sec+0003-0004",
        "sec+0004-0001", "sec+0004-0002", "sec+0004-0003", "sec+0004-0004",
    ];

    [Fact]
    public void Validate_BerlinRegion_NoStructuralErrors()
    {
        var files = BerlinRegion.Select(ReadSector).ToList();
        var g = RoadGraph.Build(files);
        var report = GraphValidator.Validate(files, g);
        // 自环/重复边/方向矛盾应为零；引用断裂仅允许边界截断（<1% 节点）
        Assert.Empty(report.SelfLoops);
        Assert.Empty(report.DuplicateEdges);
        Assert.Empty(report.DirectionMismatches);
        int totalNodes = g.NodeCount;
        Assert.True(report.BrokenNodeRefs.Count < totalNodes / 100, $"引用断裂 {report.BrokenNodeRefs.Count} 过多");
    }

    [Fact]
    public void Validate_DeadEndsExist_ButLimited()
    {
        var files = BerlinRegion.Select(ReadSector).ToList();
        var g = RoadGraph.Build(files);
        var report = GraphValidator.Validate(files, g);
        // 死端（degree==1）应 < 15% 节点（道路端点 + 边界截断）
        Assert.True(report.DeadEnds < g.NodeCount * 0.15, $"死端 {report.DeadEnds}/{g.NodeCount}");
    }
}
