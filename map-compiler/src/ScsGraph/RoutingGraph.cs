// 路由图模型（P1 计划 §28-31）：宏观节点 + 分类边（Road/JunctionMovement/Ferry/Train/ServiceAccess）。
// 与 RoadGraph 的区别：边携带语义（movement/semaphore/road class），由 RoutingGraphBuilder 从 SemanticMap 构建。
// GPL-3.0 — ETS2Nav 项目

namespace ScsGraph;

public enum RoutingEdgeKind { Road, JunctionMovement, Ferry, Train, ServiceAccess }

public sealed class RoutingGraph
{
    private readonly Dictionary<ulong, int> _nodeIndex = new();
    private readonly List<ulong> _nodeUids = new();
    private readonly List<(double X, double Y, double Z)> _positions = new();
    private readonly List<List<int>> _outEdges = new();
    private readonly List<List<int>> _inEdges = new();
    private readonly List<RoutingEdge> _edges = new();
    private readonly List<(int From, int To)> _edgeEnds = new();

    public int NodeCount => _positions.Count;
    public int EdgeCount => _edges.Count;
    public IReadOnlyList<ulong> NodeUids => _nodeUids;
    public IReadOnlyList<(double X, double Y, double Z)> Positions => _positions;

    public int GetNodeIndex(ulong uid)
    {
        if (_nodeIndex.TryGetValue(uid, out int idx)) return idx;
        throw new KeyNotFoundException($"节点不存在：{uid:x16}");
    }

    public bool TryGetNodeIndex(ulong uid, out int idx) => _nodeIndex.TryGetValue(uid, out idx);
    public IReadOnlyList<int> OutEdges(int node) => _outEdges[node];
    public RoutingEdge Edge(int edgeId) => _edges[edgeId];
    public (int From, int To) EdgeEnds(int edgeId) => _edgeEnds[edgeId];

    public int EnsureNode(ulong uid, double x, double y, double z)
    {
        if (_nodeIndex.TryGetValue(uid, out int idx)) return idx;
        idx = _positions.Count;
        _nodeIndex[uid] = idx;
        _nodeUids.Add(uid);
        _positions.Add((x, y, z));
        _outEdges.Add(new List<int>());
        _inEdges.Add(new List<int>());
        return idx;
    }

    public int AddDirected(int from, int to, RoutingEdge edge)
    {
        _outEdges[from].Add(_edges.Count);
        _inEdges[to].Add(_edges.Count);
        _edges.Add(edge);
        _edgeEnds.Add((from, to));
        return _edges.Count - 1;
    }

    /// <summary>连通分量统计（无向视角，入边+出边，degree&gt;0 节点）。</summary>
    public (int Components, int LargestComponent, int NoEdgeNodes) ConnectedComponents()
    {
        var visited = new bool[NodeCount];
        int components = 0, largest = 0, noEdge = 0;
        for (int start = 0; start < NodeCount; start++)
        {
            if (_outEdges[start].Count == 0 && _inEdges[start].Count == 0) { noEdge++; continue; }
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
                foreach (int e in _inEdges[u])
                {
                    int v = _edgeEnds[e].From;
                    if (!visited[v]) { visited[v] = true; stack.Push(v); }
                }
            }
            if (size > largest) largest = size;
        }
        return (components, largest, noEdge);
    }
}

/// <summary>路由边（P1 计划 §29）。</summary>
public sealed class RoutingEdge
{
    public required RoutingEdgeKind Kind { get; init; }
    public required ulong SourceUid { get; init; }    // road/prefab item uid
    public double Length { get; init; }
    public string? RoadLook { get; init; }
    /// <summary>JunctionMovement 边的 movement 索引（junction 内部）。</summary>
    public int? MovementId { get; init; }
    /// <summary>JunctionMovement 边的信号灯绑定（PPD SemaphoreId，-1 无）。</summary>
    public int SemaphoreId { get; init; } = -1;
    public string? SpeedClass { get; init; }
    public bool NoAiVehicles { get; init; }
    public bool GpsAvoid { get; init; }
    public bool Secret { get; init; }
}
