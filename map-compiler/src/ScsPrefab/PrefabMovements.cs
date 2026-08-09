namespace ScsPrefab;

/// <summary>JunctionMovement（P1 计划 §21）：prefab 内一条合法行车路径。
/// EntryNode/ExitNode 为 ControlNode 索引（→ prefab node 需按 Origin 映射）。</summary>
public sealed class PrefabMovement
{
    public required string PrefabToken { get; init; }
    public required int EntryCurve { get; init; }      // NavCurves 索引（入口曲线，取路径首曲线）
    public required int ExitCurve { get; init; }       // NavCurves 索引（出口曲线，取路径末曲线）
    public required byte EntryNode { get; init; }      // ControlNode 索引
    public required byte ExitNode { get; init; }
    public required byte EntryLane { get; init; }
    public required byte ExitLane { get; init; }
    public int[] CurvePath { get; init; } = Array.Empty<int>();   // 完整曲线链（含 entry/exit）
    public required float Length { get; init; }
    public int SemaphoreId { get; init; } = -1;        // 链上信号灯（出口曲线优先）
    public int PriorityModifier { get; init; }
    public bool LowProbability { get; init; }
    public required double TurnAngle { get; init; }    // 入口方向 → 出口方向转角（度，近似）

    /// <summary>转向类型（P2 需可靠区分）：-1 左转 / 0 直行 / 1 右转 / 2 U 型（角度近似）。</summary>
    public int TurnType => Math.Abs(TurnAngle) < 30 ? 0 : Math.Abs(TurnAngle) > 150 ? 2 : TurnAngle < 0 ? -1 : 1;
}

/// <summary>
/// Prefab movement 恢复器（P1 计划 §22）：prefab connectivity 完全来自 navigation 语义。
/// 网络语义（对照 NavNode 结构）：NavNode 连接图（Physical 节点 Index = ControlNode = prefab 连接点；
/// AI 节点为内部交点）。movement = Physical 起点沿连接图（每步取该连接的第一条曲线）到 Physical 终点。
/// InputLines/OutputLines 数据在真实 corpus 中不完整（已验证 mod_ger_67），不作为端点依据。
/// </summary>
public static class PrefabMovements
{
    private const int MaxDepth = 16;

    public static List<PrefabMovement> Recover(PrefabDescriptor pd, string prefabToken)
    {
        var curves = pd.NavCurves;
        // NavNode 邻接：i → (target, curves)
        var adj = new List<List<(int Target, int[] Curves)>>(pd.NavNodes.Count);
        for (int i = 0; i < pd.NavNodes.Count; i++)
        {
            var list = new List<(int, int[])>();
            foreach (var c in pd.NavNodes[i].Connections)
            {
                var valid = c.CurveIndices.Where(x => x < curves.Count).Select(x => (int)x).ToArray();
                if (valid.Length > 0) list.Add(((int)c.TargetNodeIndex, valid));
            }
            adj.Add(list);
        }
        // Physical 节点：navNode 索引 → ControlNode 索引
        var physical = new Dictionary<int, int>();
        for (int i = 0; i < pd.NavNodes.Count; i++)
            if (pd.NavNodes[i].Type == 0 && pd.NavNodes[i].Index < pd.ControlNodes.Count)
                physical[i] = pd.NavNodes[i].Index;

        var result = new List<PrefabMovement>();
        var semaphoreByCurve = BuildSemaphoreMap(curves);
        foreach (var (start, entryCtrl) in physical)
        {
            var visited = new bool[pd.NavNodes.Count];
            visited[start] = true;
            var path = new List<int>();
            Dfs(pd, start, entryCtrl, path, visited, result, prefabToken, semaphoreByCurve, adj, physical);
        }
        return result;
    }

    private static void Dfs(PrefabDescriptor pd, int node, int entryCtrl, List<int> path, bool[] visited,
        List<PrefabMovement> result, string token, Dictionary<int, int> semaphores,
        List<List<(int Target, int[] Curves)>> adj, Dictionary<int, int> physical)
    {
        foreach (var (target, curveList) in adj[node])
        {
            // 每连接取第一条曲线（车道合并；P1-06 细化多车道）
            int curve = curveList[0];
            bool targetPhysical = physical.TryGetValue(target, out int exitCtrl);
            if (visited[target]) continue;
            if (path.Count + 1 >= MaxDepth) continue;
            visited[target] = true;
            path.Add(curve);
            if (targetPhysical)
            {
                var first = pd.NavCurves[path[0]];
                var last = pd.NavCurves[curve];
                result.Add(new PrefabMovement
                {
                    PrefabToken = token,
                    EntryCurve = path[0],
                    ExitCurve = curve,
                    EntryNode = (byte)entryCtrl,
                    ExitNode = (byte)exitCtrl,
                    EntryLane = first.StartLane,
                    ExitLane = last.EndLane,
                    CurvePath = path.ToArray(),
                    Length = path.Sum(i => pd.NavCurves[i].Length),
                    SemaphoreId = FindSemaphore(path, semaphores),
                    PriorityModifier = first.PriorityModifier,
                    LowProbability = first.LowProbability,
                    TurnAngle = TurnAngleBetween(first, last),
                });
            }
            Dfs(pd, target, entryCtrl, path, visited, result, token, semaphores, adj, physical);
            path.RemoveAt(path.Count - 1);
            visited[target] = false;
        }
    }

    /// <summary>每条曲线所属信号灯（SemaphoreId → 最近前缀曲线，链上前缀优先）。</summary>
    private static Dictionary<int, int> BuildSemaphoreMap(List<NavCurveData> curves)
    {
        var map = new Dictionary<int, int>();
        for (int i = 0; i < curves.Count; i++)
            if (curves[i].SemaphoreId >= 0) map[i] = curves[i].SemaphoreId;
        return map;
    }

    private static int FindSemaphore(List<int> path, Dictionary<int, int> semaphores)
    {
        // 优先出口附近的信号灯（从后向前找）
        for (int i = path.Count - 1; i >= 0; i--)
            if (semaphores.TryGetValue(path[i], out var id)) return id;
        return -1;
    }

    /// <summary>入口方向 → 出口方向转角（基于曲线端点几何，atan2 叉积近似）。</summary>
    private static double TurnAngleBetween(NavCurveData entry, NavCurveData exit)
    {
        double ex = entry.EndX - entry.StartX, ez = entry.EndZ - entry.StartZ;
        double xx = exit.EndX - exit.StartX, xz = exit.EndZ - exit.StartZ;
        double eAng = Math.Atan2(ez, ex);
        double xAng = Math.Atan2(xz, xx);
        double d = xAng - eAng;
        while (d > Math.PI) d -= 2 * Math.PI;
        while (d < -Math.PI) d += 2 * Math.PI;
        return d * 180 / Math.PI;
    }
}
