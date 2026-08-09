// 道路图模型（v0.2 §14：紧凑结构）。
// P0 简化：节点 = MapNode；边 = Road item（双向，方向细化待 road_look 车道数据）；
// prefab 通过共享节点连接（prefab 内部连通性待 navigation path 细化）。
// GPL-3.0 — ETS2Nav 项目

using ScsSector;

namespace ScsGraph;

public sealed class RoadGraph
{
    /// <summary>节点：uid → 内部索引（连续）。</summary>
    private readonly Dictionary<ulong, int> _nodeIndex = new();
    private readonly List<ulong> _nodeUids = new();
    private readonly List<(double X, double Y, double Z)> _positions = new();
    /// <summary>有向边（双向道路存两条）。from → to 列表。</summary>
    private readonly List<List<int>> _outEdges = new();
    /// <summary>边元数据（按 _outEdges 顺序）。</summary>
    private readonly List<EdgeData> _edgeData = new();
    private readonly List<(int From, int To)> _edgeEnds = new();

    public int NodeCount => _positions.Count;
    public int EdgeCount => _edgeData.Count;

    /// <summary>节点 UID 表（索引与 Positions 对齐，供调试/导出）。</summary>
    public IReadOnlyList<ulong> NodeUids => _nodeUids;

    public IReadOnlyList<(double X, double Y, double Z)> Positions => _positions;

    public int GetNodeIndex(ulong uid)
    {
        if (_nodeIndex.TryGetValue(uid, out int idx)) return idx;
        throw new KeyNotFoundException($"节点不存在：{uid:x16}");
    }

    public bool TryGetNodeIndex(ulong uid, out int idx) => _nodeIndex.TryGetValue(uid, out idx);

    public IReadOnlyList<int> OutEdges(int node) => _outEdges[node];
    public EdgeData Edge(int edgeId) => _edgeData[edgeId];
    public (int From, int To) EdgeEnds(int edgeId) => _edgeEnds[edgeId];

    public sealed class EdgeData
    {
        public required string RoadLook { get; init; }
        public required ulong ItemUid { get; init; }
        public double Length { get; init; }
        public bool IsPrefabConnector { get; init; }
    }

    private int EnsureNode(ulong uid, double x, double y, double z)
    {
        if (_nodeIndex.TryGetValue(uid, out int idx)) return idx;
        idx = _positions.Count;
        _nodeIndex[uid] = idx;
        _nodeUids.Add(uid);
        _positions.Add((x, y, z));
        _outEdges.Add(new List<int>());
        return idx;
    }

    /// <summary>加有向边（调用方决定双向）。</summary>
    private int AddDirected(int from, int to, EdgeData data)
    {
        _outEdges[from].Add(_edgeData.Count);
        _edgeData.Add(data);
        _edgeEnds.Add((from, to));
        return _edgeData.Count - 1;
    }

    /// <summary>从 sector 集合构建图。双向道路生成两条边；prefab 节点间全连接（保守近似，待 navigation path 细化）。</summary>
    public static RoadGraph Build(IEnumerable<SectorFile> sectors)
    {
        var g = new RoadGraph();
        var sectorsList = sectors.ToList();

        // 第一遍：全局节点表（跨 sector 节点合并）
        foreach (var sec in sectorsList)
            foreach (var n in sec.Nodes)
                g.EnsureNode(n.Uid, n.X, n.Y, n.Z);

        // 第二遍：road 边（用全局节点表）
        foreach (var sec in sectorsList)
        {
            foreach (var road in sec.Roads)
            {
                if (!g.TryGetNodeIndex(road.Node0, out int a) || !g.TryGetNodeIndex(road.Node1, out int b))
                    continue;
                var data = new EdgeData { RoadLook = road.RoadLook, ItemUid = road.Uid, Length = road.Length };
                g.AddDirected(a, b, data);
                g.AddDirected(b, a, data);   // P0：双向近似
            }
        }

        // 第三遍：prefab 节点全连接（同一 prefab 的节点彼此连通，保守近似）
        foreach (var sec in sectorsList)
        {
            foreach (var prefab in sec.Prefabs)
            {
                var indices = new List<int>();
                foreach (var nuid in prefab.NodeUids)
                    if (g.TryGetNodeIndex(nuid, out int idx))
                        indices.Add(idx);
                for (int i = 0; i < indices.Count; i++)
                {
                    for (int j = i + 1; j < indices.Count; j++)
                    {
                        var data = new EdgeData
                        {
                            RoadLook = "", ItemUid = prefab.Uid, IsPrefabConnector = true,
                            Length = Distance(g._positions[indices[i]], g._positions[indices[j]]),
                        };
                        g.AddDirected(indices[i], indices[j], data);
                        g.AddDirected(indices[j], indices[i], data);
                    }
                }
            }
        }

        return g;
    }

    private static double Distance((double X, double Y, double Z) a, (double X, double Y, double Z) b)
    {
        double dx = a.X - b.X, dy = a.Y - b.Y, dz = a.Z - b.Z;
        return Math.Sqrt(dx * dx + dy * dy + dz * dz);
    }

    /// <summary>连通分量统计（无向视角，仅统计 degree&gt;0 的节点）。返回（分量数，最大分量节点数，无道路边节点数）。</summary>
    public (int Components, int LargestComponent, int NoRoadEdges) ConnectedComponents()
    {
        var visited = new bool[NodeCount];
        int components = 0, largest = 0, noRoad = 0;
        for (int start = 0; start < NodeCount; start++)
        {
            if (_outEdges[start].Count == 0) { noRoad++; continue; }
            if (visited[start]) continue;
            components++;
            var stack = new Stack<int>();
            stack.Push(start);
            visited[start] = true;
            int size = 0;
            while (stack.Count > 0)
            {
                int u = stack.Pop();
                size++;
                foreach (int e in _outEdges[u])
                {
                    int v = _edgeEnds[e].To;
                    if (!visited[v]) { visited[v] = true; stack.Push(v); }
                }
            }
            if (size > largest) largest = size;
        }
        return (components, largest, noRoad);
    }

    /// <summary>BFS 可达性：from 能否到达 to。</summary>
    public bool Reachable(int from, int to)
    {
        if (from == to) return true;
        var visited = new bool[NodeCount];
        var queue = new Queue<int>();
        queue.Enqueue(from);
        visited[from] = true;
        while (queue.Count > 0)
        {
            int u = queue.Dequeue();
            foreach (int e in _outEdges[u])
            {
                int v = _edgeEnds[e].To;
                if (v == to) return true;
                if (!visited[v]) { visited[v] = true; queue.Enqueue(v); }
            }
        }
        return false;
    }
}
