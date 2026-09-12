using ScsSector;
using ScsGraph;
using ScsTests;

namespace ScsGraph.Tests;

public class RoadGraphTests
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

    private static readonly string[] BerlinCore =
        ["sec+0002-0002", "sec+0002-0003", "sec+0003-0002", "sec+0003-0003"];

    private static RoadGraph BuildCore() => RoadGraph.Build(BerlinCore.Select(ReadSector));

    [Fact]
    public void Build_HasRoadNodesAndEdges()
    {
        var g = BuildCore();
        Assert.True(g.NodeCount > 3000);
        Assert.True(g.EdgeCount > 4000);
    }

    [Fact]
    public void Build_PrefabSharesRoadNodes()
    {
        // 核心验证：prefab 与 road 通过共享节点连接（同 uid 合并）
        var sec = ReadSector("sec+0002-0003");
        var prefab = sec.Prefabs.First();
        var g = BuildCore();
        int connected = 0;
        foreach (var nuid in prefab.NodeUids)
        {
            if (g.TryGetNodeIndex(nuid, out int idx) && g.OutEdges(idx).Count > 0)
                connected++;
        }
        // 至少一个 prefab 节点有边（与 road 或 prefab 内部连接）
        Assert.True(connected > 0);
    }

    [Fact]
    public void ConnectedComponents_MainComponentDominates()
    {
        var g = BuildCore();
        var (comp, largest, noRoad) = g.ConnectedComponents();
        int roadNodes = g.NodeCount - noRoad;
        // 主分量覆盖大部分道路节点（边界截断允许 ~15% 外溢）
        Assert.True(largest >= roadNodes * 0.8, $"主分量 {largest} < 道路节点 80% ({roadNodes})");
    }

    [Fact]
    public void RandomOdPairs_AllReachableInMainComponent()
    {
        var g = BuildCore();
        var (_, _, noRoad) = g.ConnectedComponents();
        var roadNodes = Enumerable.Range(0, g.NodeCount).Where(i => g.OutEdges(i).Count > 0).ToList();
        if (roadNodes.Count == 0) return;

        // 主分量
        var visited = new bool[g.NodeCount];
        var stack = new Stack<int>();
        stack.Push(roadNodes[0]);
        visited[roadNodes[0]] = true;
        while (stack.Count > 0)
        {
            int u = stack.Pop();
            foreach (int e in g.OutEdges(u))
            {
                int v = g.EdgeEnds(e).To;
                if (!visited[v]) { visited[v] = true; stack.Push(v); }
            }
        }
        var main = roadNodes.Where(i => visited[i]).ToList();
        if (main.Count < 100) return;
        var rnd = new Random(42);
        for (int i = 0; i < 100; i++)
        {
            int a = main[rnd.Next(main.Count)];
            int b = main[rnd.Next(main.Count)];
            Assert.True(g.Reachable(a, b), $"OD 不可达：节点 {a} → {b}");
        }
    }

    [Fact]
    public void Reachable_SelfAndNeighbor()
    {
        var g = BuildCore();
        // 找一条边
        for (int u = 0; u < g.NodeCount; u++)
        {
            var edges = g.OutEdges(u);
            if (edges.Count > 0)
            {
                int v = g.EdgeEnds(edges[0]).To;
                Assert.True(g.Reachable(u, u));
                Assert.True(g.Reachable(u, v));
                Assert.True(g.Reachable(v, u));   // 双向近似
                return;
            }
        }
        Assert.Fail("图中无任何边");
    }
}
