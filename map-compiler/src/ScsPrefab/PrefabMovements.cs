namespace ScsPrefab;

/// <summary>JunctionMovement（P1 计划 §21）：prefab 内一条合法行车路径。</summary>
public sealed class PrefabMovement
{
    public required string PrefabToken { get; init; }
    public required int EntryCurve { get; init; }      // NavCurves 索引（入口曲线）
    public required int ExitCurve { get; init; }       // NavCurves 索引（出口曲线）
    public required byte EntryNode { get; init; }      // prefab node（连接外部道路）
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
/// Prefab movement 恢复器（P1 计划 §22）：prefab connectivity 完全来自 navigation 语义——
/// 从每个入口曲线沿 NextLines 链深度遍历到出口曲线，每条完整路径 = 一个 movement。
/// </summary>
public static class PrefabMovements
{
    private const int MaxDepth = 12;

    public static List<PrefabMovement> Recover(PrefabDescriptor pd, string prefabToken)
    {
        var curves = pd.NavCurves;
        var result = new List<PrefabMovement>();
        var semaphoreByCurve = BuildSemaphoreMap(curves);
        for (int entry = 0; entry < curves.Count; entry++)
        {
            var c = curves[entry];
            if (!c.IsEntry) continue;   // 只从入口曲线出发
            var visited = new bool[curves.Count];
            visited[entry] = true;
            var path = new List<int> { entry };
            Dfs(pd, entry, path, visited, entry, result, prefabToken, semaphoreByCurve);
        }
        return result;
    }

    /// <summary>每条曲线所属信号灯（SemaphoreId → 最近前缀曲线，链上前缀优先）。</summary>
    private static Dictionary<int, int> BuildSemaphoreMap(List<NavCurveData> curves)
    {
        var map = new Dictionary<int, int>();
        for (int i = 0; i < curves.Count; i++)
            if (curves[i].SemaphoreId >= 0) map[i] = curves[i].SemaphoreId;
        return map;
    }

    private static void Dfs(PrefabDescriptor pd, int entry, List<int> path, bool[] visited,
        int curve, List<PrefabMovement> result, string token, Dictionary<int, int> semaphores)
    {
        var c = pd.NavCurves[curve];
        if (c.IsExit && path.Count > 1)
        {
            var first = pd.NavCurves[entry];
            var last = c;
            result.Add(new PrefabMovement
            {
                PrefabToken = token,
                EntryCurve = entry,
                ExitCurve = curve,
                EntryNode = first.StartNode,
                ExitNode = last.EndNode,
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
        if (path.Count >= MaxDepth) return;
        for (int k = 0; k < c.NextCount; k++)
        {
            int next = c.NextLines[k];
            if (next < 0 || next >= pd.NavCurves.Count || visited[next]) continue;
            visited[next] = true;
            path.Add(next);
            Dfs(pd, entry, path, visited, next, result, token, semaphores);
            path.RemoveAt(path.Count - 1);
            visited[next] = false;
        }
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
