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

// ---- TL-03 Warp 检测：sim 跳变（Δsim 与 wall×scale 期望偏差 > 300ms 且非暂停）----
Console.WriteLine("\n=== TL-03 Warp 检测（B7 实验：warp 下信号灯行为） ===");
var warps = new List<(long WallMs, double JumpMs)>();
for (int i = 1; i < samples.Count; i++)
{
    var a = samples[i - 1];
    var b = samples[i];
    double dwallMs = b.WallMs - a.WallMs;
    if (dwallMs <= 0 || dwallMs > 5000) continue;          // 采样间隙忽略
    double expectedUs = dwallMs * a.Scale * 1000.0;         // 期望 sim 推进
    double actualUs = (double)(b.SimUs > a.SimUs ? b.SimUs - a.SimUs : 0);
    double jumpMs = (actualUs - expectedUs) / 1000.0;
    if (jumpMs > 300.0)
        warps.Add((b.WallMs, jumpMs));
}
if (warps.Count > 0)
{
    Console.WriteLine($"检出 sim 跳变 {warps.Count} 处（>300ms 超额推进）：");
    foreach (var w in warps.Take(10))
        Console.WriteLine($"  wall={w.WallMs / 1000.0:F1}s 超额 {w.JumpMs:F0}ms —— warp 候选");
}
else Console.WriteLine("无 sim 跳变（未 warp 或数据无 warp 段）");

// ---- TL-04 Reset 检测：sim 倒退（读档/快速旅行重置）----
Console.WriteLine("\n=== TL-04 Reset 检测（load/quick travel/teleport 后相位重置） ===");
var resets = new List<(long WallMs, ulong FromUs, ulong ToUs)>();
for (int i = 1; i < samples.Count; i++)
{
    if (samples[i].SimUs < samples[i - 1].SimUs)
        resets.Add((samples[i].WallMs, samples[i - 1].SimUs, samples[i].SimUs));
}
if (resets.Count > 0)
{
    Console.WriteLine($"检出 sim 倒退 {resets.Count} 处：");
    foreach (var r in resets.Take(10))
        Console.WriteLine($"  wall={r.WallMs / 1000.0:F1}s sim {r.FromUs / 1_000_000.0:F1}s → {r.ToUs / 1_000_000.0:F1}s —— reset/快速旅行候选");
}
else Console.WriteLine("无 sim 倒退（无 reset 或数据无该段）");

// ---- TL-05 特殊 profile：手动标记（采集侧 P 键）统计 ----
var profs = events.Where(e => e.Kind == "PROF").ToList();
if (profs.Count > 0)
    Console.WriteLine($"\n=== TL-05 特殊 profile 路口标记：{profs.Count} 处（采集侧 P 键）——与事件时刻的相位/时钟对照分析 ===");
var rstMarks = events.Where(e => e.Kind == "RST").ToList();
if (rstMarks.Count > 0)
    Console.WriteLine($"\n=== TL-04 手动标记（R 键）：{rstMarks.Count} 处——与自动检测对照 ===");

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
        // 列：wall_ms, sim_us, paused_sim_us, render_us, game_min, scale, speed_kph
        list.Add((long.Parse(p[0]), ulong.Parse(p[1]), ulong.Parse(p[2]), uint.Parse(p[4]), float.Parse(p[5], CultureInfo.InvariantCulture)));
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
