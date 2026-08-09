// signal-analyze：TL-01 Clock Domain / TL-02 Phase Anchor 数据分析（v0.2 §29-30）。
//
// TL-01：对同一路口的连续相位转换事件对，计算
//   Δt_wall（真实秒）、Δt_sim（游戏微秒→游戏秒）、实测倍率 = Δt_sim / Δt_wall
// 判定：
//   - 实测倍率 ≈ scale（如 3.0）→ 信号灯由 simulation 时钟驱动（interval 是模拟秒）
//   - 实测倍率 ≈ 1.0 → 信号灯由真实时间驱动（interval 是真实秒）
//   - 其他稳定比例 → 其他时钟域
//
// TL-02：同一路口多次进入（多次会话），比较相位转换事件在
//   game.time（全局游戏时钟）下的相位位置是否稳定：
//   - 稳定（固定相位偏移）→ H1 全局时钟模型
//   - 不稳定 → H2 局部加载锚定模型
//
// 用法：signal-analyze <samples.csv> <events.csv> [--dump]

using System.Globalization;

if (args.Length < 2)
{
    Console.WriteLine("用法: signal-analyze <samples.csv> <events.csv> [--dump]");
    return;
}

var samples = LoadSamples(args[0]);
var events = LoadEvents(args[1]);
Console.WriteLine($"样本 {samples.Count} 条，事件 {events.Count} 条");

if (events.Count < 2)
{
    Console.WriteLine("事件不足（<2），无法分析。请在同一路口记录至少两个连续转换。");
    return;
}

// ---- TL-01：相邻事件对的间隔 ----
Console.WriteLine("\n=== TL-01 Clock Domain：相邻转换间隔 ===");
Console.WriteLine("事件对 | Δwall(s) | Δsim(s) | 实测倍率 | 同时段scale | 判定");
for (int i = 0; i < events.Count - 1; i++)
{
    var a = events[i];
    var b = events[i + 1];
    double dwall = (b.WallMs - a.WallMs) / 1000.0;
    double dsim = (b.SimUs - a.SimUs) / 1_000_000.0;
    double ratio = dwall > 0 ? dsim / dwall : 0;
    var midSamples = samples.Where(s => s.WallMs >= a.WallMs && s.WallMs <= b.WallMs).ToList();
    double avgScale = midSamples.Count > 0 ? midSamples.Average(s => s.Scale) : a.Scale;
    string verdict;
    if (dwall <= 0.05) verdict = "瞬时/误触";
    else if (Math.Abs(ratio - avgScale) / avgScale < 0.15) verdict = "simulation 时钟驱动（模拟秒）";
    else if (Math.Abs(ratio - 1.0) < 0.15) verdict = "真实时间驱动（真实秒）";
    else if (ratio < 0.5) verdict = "疑似误触/快速闪烁";
    else verdict = $"其他时钟域（倍率 {ratio:F2}）";
    Console.WriteLine($"{a.Kind}->{b.Kind} | {dwall,8:F2} | {dsim,8:F2} | {ratio,8:F3} | {avgScale,8:F3} | {verdict}");
}

// ---- 相同类型事件对的周期估计 ----
var sameKind = new List<(double Dt, double Ratio, double Scale)>();
for (int i = 0; i < events.Count - 1; i++)
{
    if (events[i].Kind == events[i + 1].Kind && events[i].Kind != "F9" && events[i].Kind != "OTHER")
    {
        double dwall = (events[i + 1].WallMs - events[i].WallMs) / 1000.0;
        double dsim = (events[i + 1].SimUs - events[i].SimUs) / 1_000_000.0;
        sameKind.Add((dwall, dsim / dwall, events[i].Scale));
    }
}
if (sameKind.Count > 0)
{
    Console.WriteLine($"\n同类型事件间隔（周期候选）：{sameKind.Count} 对");
    foreach (var p in sameKind)
        Console.WriteLine($"  Δwall={p.Dt:F2}s 实测倍率={p.Ratio:F3}（scale={p.Scale:F3}）");
}

// ---- TL-02：相位 vs 全局游戏时钟 ----
Console.WriteLine("\n=== TL-02 Phase Anchor：事件时刻的游戏时钟相位 ===");
Console.WriteLine("（同一路口多次进入时，事件对应 game.time 的相位应稳定 → H1；否则 → H2）");
var phaseList = events
    .Where(e => e.Kind is "R2G" or "G2R")
    .Select(e => (e.GameMin, e.WallMs, e.Kind))
    .ToList();
foreach (var p in phaseList)
    Console.WriteLine($"  {p.Kind} @ wall={p.WallMs / 1000.0:F1}s game={p.GameMin}min");

Console.WriteLine("\n说明：比较各次进入同一路口的转换时刻。若 game.time 对固定周期取模后相位一致 → 全局时钟；");
Console.WriteLine("若相位随进入时刻变化 → 加载锚定。固定周期可从 semaphore_profile 的 interval 总和获得。");

// ---- 可选：样本 dump ----
if (args.Contains("--dump"))
{
    Console.WriteLine("\n=== 样本前 20 条 ===");
    foreach (var s in samples.Take(20))
        Console.WriteLine($"  wall={s.WallMs}ms sim={s.SimUs / 1_000_000.0:F3}s paused={s.PausedUs / 1_000_000.0:F3}s game={s.GameMin}min scale={s.Scale:F3}");
}

static List<(long WallMs, ulong SimUs, ulong PausedUs, uint GameMin, float Scale)> LoadSamples(string path)
{
    var list = new List<(long, ulong, ulong, uint, float)>();
    foreach (var line in File.ReadLines(path).Skip(1))
    {
        var p = line.Split(',');
        if (p.Length < 6) continue;
        list.Add((long.Parse(p[0]), ulong.Parse(p[1]), ulong.Parse(p[2]), uint.Parse(p[3]), float.Parse(p[5], CultureInfo.InvariantCulture)));
    }
    return list;
}

static List<(long WallMs, ulong SimUs, uint GameMin, float Scale, string Kind)> LoadEvents(string path)
{
    var list = new List<(long, ulong, uint, float, string)>();
    foreach (var line in File.ReadLines(path).Skip(1))
    {
        var p = line.Split(',');
        if (p.Length < 6) continue;
        list.Add((long.Parse(p[0]), ulong.Parse(p[1]), uint.Parse(p[3]), float.Parse(p[4], CultureInfo.InvariantCulture), p[5]));
    }
    return list;
}
