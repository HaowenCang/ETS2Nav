// ScsValidation 单元测试。
// GPL-3.0 — ETS2Nav 项目

using ScsSector;
using ScsValidation;
using ScsValidation.Validators;
using Xunit;

namespace ScsValidation.Tests;

public class ValidationEngineTests
{
    private static SectorFile Sector(params MapItem[] items)
        => new()
        {
            GameId = "eut2",
            Items = items.ToList(),
            Nodes = new List<MapNode>(),
            VisibilityAreaUids = new List<ulong>(),
            SectorName = "sec+0000+0000",
        };

    private static MapItem Item(ulong uid) => new RoadItem
    {
        Type = ScsSector.ItemType.Road,
        Uid = uid,
        RoadLook = "road_look_test",
        RightLanes = "1",
        LeftLanes = "1",
        Node0 = 0,
        Node1 = 0,
    };

    private static RoadItem Road(ulong uid, ulong n0, ulong n1) => new RoadItem
    {
        Type = ScsSector.ItemType.Road,
        Uid = uid,
        RoadLook = "road_look_test",
        RightLanes = "1",
        LeftLanes = "1",
        Node0 = n0,
        Node1 = n1,
    };

    private static ValidationEngine Engine() => new ValidationEngine()
        .Register(new StructuralValidator())
        .Register(new ReferenceValidator())
        .Register(new DirectionValidator())
        .Register(new ConnectivityValidator())
        .Register(new GeometryValidator());

    [Fact]
    public void DuplicateUid_IsError()
    {
        var sec = Sector(Item(0xAA), Item(0xAA));
        var graph = ScsGraph.RoadGraph.Build(new[] { sec });
        var report = Engine().Run(new ValidationContext { Sectors = new[] { sec }, Graph = graph });
        Assert.Contains(report.Issues, i => i.Code == "STRUCT_DUP_UID" && i.Severity == ValidationSeverity.Error);
    }

    [Fact]
    public void BrokenNodeRef_IsError()
    {
        var sec = new SectorFile
        {
            GameId = "eut2",
            Items = new List<MapItem> { Item(0xAA) },
            Nodes = new List<MapNode>
            {
                new() { Uid = 0x01, X = 0, Y = 0, Z = 0, BackwardItemUid = 0xBB /* 不存在 */, ForwardItemUid = 0xAA },
            },
            VisibilityAreaUids = new List<ulong>(),
            SectorName = "sec+0000+0000",
        };
        var graph = ScsGraph.RoadGraph.Build(new[] { sec });
        var report = Engine().Run(new ValidationContext { Sectors = new[] { sec }, Graph = graph });
        Assert.Contains(report.Issues, i => i.Code == "REF_BROKEN_BACKWARD" && i.Severity == ValidationSeverity.Error);
    }

    [Fact]
    public void SelfLoop_IsError()
    {
        var sec = new SectorFile
        {
            GameId = "eut2",
            Items = new List<MapItem>
            {
                Road(0xAA, 0x10, 0x10),
            },
            Nodes = new List<MapNode>
            {
                new() { Uid = 0x10, X = 1, Y = 0, Z = 1, BackwardItemUid = 0xAA, ForwardItemUid = 0xAA },
            },
            VisibilityAreaUids = new List<ulong>(),
            SectorName = "sec+0000+0000",
        };
        var graph = ScsGraph.RoadGraph.Build(new[] { sec });
        var report = Engine().Run(new ValidationContext { Sectors = new[] { sec }, Graph = graph });
        Assert.Contains(report.Issues, i => i.Code == "STRUCT_SELF_LOOP" && i.Severity == ValidationSeverity.Error);
    }

    [Fact]
    public void ZeroLengthEdge_IsWarning()
    {
        var sec = new SectorFile
        {
            GameId = "eut2",
            Items = new List<MapItem>
            {
                Road(0xAA, 0x10, 0x11),
            },
            Nodes = new List<MapNode>
            {
                new() { Uid = 0x10, X = 1, Y = 0, Z = 1, BackwardItemUid = 0, ForwardItemUid = 0xAA },
                new() { Uid = 0x11, X = 1, Y = 0, Z = 1, BackwardItemUid = 0xAA, ForwardItemUid = 0 },
            },
            VisibilityAreaUids = new List<ulong>(),
            SectorName = "sec+0000+0000",
        };
        var graph = ScsGraph.RoadGraph.Build(new[] { sec });
        var report = Engine().Run(new ValidationContext { Sectors = new[] { sec }, Graph = graph });
        Assert.Contains(report.Issues, i => i.Code == "GEOM_ZERO_LENGTH" && i.Severity == ValidationSeverity.Warning);
    }

    [Fact]
    public void Connectivity_ReportsStats()
    {
        var sec = new SectorFile
        {
            GameId = "eut2",
            Items = new List<MapItem>
            {
                Road(0xAA, 0x10, 0x11),
            },
            Nodes = new List<MapNode>
            {
                new() { Uid = 0x10, X = 0, Y = 0, Z = 0, BackwardItemUid = 0, ForwardItemUid = 0xAA },
                new() { Uid = 0x11, X = 10, Y = 0, Z = 0, BackwardItemUid = 0xAA, ForwardItemUid = 0 },
            },
            VisibilityAreaUids = new List<ulong>(),
            SectorName = "sec+0000+0000",
        };
        var graph = ScsGraph.RoadGraph.Build(new[] { sec });
        var report = Engine().Run(new ValidationContext { Sectors = new[] { sec }, Graph = graph });
        var stats = report.Issues.First(i => i.Code == "CONN_STATS");
        Assert.Contains("连通分量 1", stats.Description);
        Assert.Contains("100.0%", stats.Description);
    }

    [Fact]
    public void JsonExport_MatchesSchema()
    {
        var sec = Sector(Item(0xAA), Item(0xAA));
        var graph = ScsGraph.RoadGraph.Build(new[] { sec });
        var report = Engine().Run(new ValidationContext { Sectors = new[] { sec }, Graph = graph });
        var json = report.ToJson();
        Assert.Contains("\"errors\": 1", json);
        Assert.Contains("\"STRUCT_DUP_UID\"", json);
    }
}
