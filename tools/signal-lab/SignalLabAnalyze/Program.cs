// signal-analyze：TL-01 Clock Domain / TL-02 Phase Anchor / TL-03 Warp / TL-04 Reset /
// TL-05 Special Profiles 数据分析（v0.2 §29-30、§64；A5 审计修复 2026-08-12）。
//
// TL-01：对同一路口的连续相位转换事件对，计算 Δt_wall、Δt_sim、实测倍率。
//   审计修复：RST/PROF 标记事件不参与相邻对分析（跨标记对会误导判定）；
//   Δsim 用有符号差（读档倒退不再 ulong 下溢）。
// TL-02：相位 vs 全局游戏时钟（H1/H2）。
// TL-03：sim 跳变（快速旅行/休息跳过/时间加速触发）自动检测 + warp 点前后最近相位事件对照。
//   注意（审计修正）：ETS2 位置传送（goto）不改变 sim 时钟——不会被检出；
//   该场景请用 R 键手动标记（分析侧按 RST 对照）。
// TL-04：sim 倒退（读档）自动检测 + RST 手动标记 + RST 后相位事件对照。
// TL-05：PROF 手动标记 + 标记时刻相位/时钟对照（判定归 B3 人工——工具只出证据）。
//
// 用法：SignalLabAnalyze <samples.csv> <events.csv> [--dump]

using System.Globalization;

if (args.Length < 2)
{
    Console.WriteLine("用法: SignalLabAnalyze <samples.csv> <events.csv> [--dump]");
    return;
}

var samples = LoadSamples(args[0]);
var events = LoadEvents(args[1]);
Console.WriteLine($"样本 {samples.Count} 条，事件 {events.Count} 条");

// ---- TL-01：相邻事件对的间隔（跳过标记类 RST/PROF——审计 MAJOR-1）----
Console.WriteLine("\n=== TL-01 Clock Domain：相邻转换间隔 ===");
Console.WriteLine("事件对 | Δwall(s) | Δsim(s) | 实测倍率 | 同时段scale | 判定");
if (events.Count < 2)
{
    Console.WriteLine("事件不足（<2），无法进行 TL-01 分析——TL-03/04/05 仍基于 samples 继续。");
}
else
{
    var phaseEvents = events.Where(e => e.Kind != "RST" && e.Kind != "PROF").ToList();
    for (int i = 0; i < phaseEvents.Count - 1; i++)
    {
        var a = phaseEvents[i];
        var b = phaseEvents[i + 1];
        double dwall = (b.WallMs - a.WallMs) / 1000.0;
        double dsim = ((long)b.SimUs - (long)a.SimUs) / 1_000_000.0; // 有符号差（审计修复）
        double ratio = dwall > 0 ? dsim / dwall : 0;
        var midSamples = samples.Where(s => s.WallMs >= a.WallMs && s.WallMs <= b.WallMs).ToList();
        double avgScale = midSamples.Count > 0 ? midSamples.Average(s => s.Scale) : a.Scale;
        string verdict;
        if (dwall <= 0.05) verdict = "瞬时/误触";
        else if (Math.Abs(ratio - avgScale) / avgScale < 0.15) verdict = "simulation 时钟驱动（模拟秒）";
        else if (Math.Abs(ratio - 1.0) < 0.15) verdict = "真实时间驱动（真实秒）";
        else if (ratio < 0.5) verdict = "疑似误触/快速闪烁/跨重置";
        else verdict = $"其他时钟域（倍率 {ratio:F2}）";
        Console.WriteLine($"{a.Kind}->{b.Kind} | {dwall,8:F2} | {dsim,8:F2} | {ratio,8:F3} | {avgScale,8:F3} | {verdict}");
    }
    // 同类型相位事件对的周期估计
    var sameKind = new List<(double Dt, double Ratio, double Scale)>();
    for (int i = 0; i < phaseEvents.Count - 1; i++)
    {
        if (phaseEvents[i].Kind == phaseEvents[i + 1].Kind && phaseEvents[i].Kind != "F9" && phaseEvents[i].Kind != "OTHER")
        {
            double dwall = (phaseEvents[i + 1].WallMs - phaseEvents[i].WallMs) / 1000.0;
            double dsim = ((long)phaseEvents[i + 1].SimUs - (long)phaseEvents[i].SimUs) / 1_000_000.0;
            sameKind.Add((dwall, dsim / dwall, phaseEvents[i].Scale));
        }
    }
    if (sameKind.Count > 0)
    {
        Console.WriteLine($"\n同类型事件间隔（周期候选）：{sameKind.Count} 对");
        foreach (var p in sameKind)
            Console.WriteLine($"  Δwall={p.Dt:F2}s 实测倍率={p.Ratio:F3}（scale={p.Scale:F3}）");
    }
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

// ---- TL-03 Warp 检测（审计修复：判定输出移到 events 门槛之前；对照相位事件）----
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
    Console.WriteLine($"检出 sim 跳变 {warps.Count} 处（>300ms 超额推进；触发源=快速旅行/休息跳过/时间加速——位置传送不改变 sim，不可检出）:");
    foreach (var w in warps.Take(10))
    {
        Console.WriteLine($"  wall={w.WallMs / 1000.0:F1}s 超额 {w.JumpMs:F0}ms —— warp 候选");
        // 审计 MAJOR-1（a5-spec）：warp 点前后最近相位事件对照（判定归 B3 人工）
        var near = events
            .Where(e => e.Kind is "R2G" or "G2R" or "OTHER" or "F9")
            .Select(e => (e, Dist: Math.Abs(e.WallMs - w.WallMs)))
            .OrderBy(x => x.Dist)
            .Take(2)
            .ToList();
        foreach (var (e, dist) in near)
            Console.WriteLine($"    对照相位事件: {e.Kind} @ wall={e.WallMs / 1000.0:F1}s（距 warp {dist / 1000.0:F1}s）");
    }
}
else Console.WriteLine("无 sim 跳变（未 warp 或数据无 warp 段）");

// ---- TL-04 Reset 检测（审计修复：倒退无阈值——任意倒退即计数；对照 RST 后相位）----
Console.WriteLine("\n=== TL-04 Reset 检测（load/quick travel/teleport 后相位重置） ===");
var resets = new List<(long WallMs, ulong FromUs, ulong ToUs)>();
for (int i = 1; i < samples.Count; i++)
{
    if (samples[i].SimUs < samples[i - 1].SimUs)
        resets.Add((samples[i].WallMs, samples[i - 1].SimUs, samples[i].SimUs));
}
if (resets.Count > 0)
{
    Console.WriteLine($"检出 sim 倒退 {resets.Count} 处（读档触发；快速旅行使 sim 前进属 TL-03 机制）:");
    foreach (var r in resets.Take(10))
    {
        Console.WriteLine($"  wall={r.WallMs / 1000.0:F1}s sim {r.FromUs / 1_000_000.0:F1}s → {r.ToUs / 1_000_000.0:F1}s —— reset 候选");
        // 审计 MAJOR-2（a5-spec）：RST 后相位事件对照（倒退后相位归零/保持 → B3 判定）
        var after = events
            .Where(e => e.WallMs >= r.WallMs && e.Kind is "R2G" or "G2R")
            .OrderBy(e => e.WallMs)
            .Take(3)
            .ToList();
        foreach (var e in after)
            Console.WriteLine($"    reset 后相位事件: {e.Kind} @ wall={e.WallMs / 1000.0:F1}s sim={e.SimUs / 1_000_000.0:F1}s");
    }
}
else Console.WriteLine("无 sim 倒退（无 reset 或数据无该段）");

// ---- TL-05 特殊 profile：手动标记（采集侧 P 键）统计 + 相位对照（审计 MAJOR-3）----
var profs = events.Where(e => e.Kind == "PROF").ToList();
Console.WriteLine("\n=== TL-05 特殊 profile 路口标记 ===");
if (profs.Count > 0)
{
    Console.WriteLine($"标记 {profs.Count} 处（采集侧 P 键）——标记时刻相位/时钟证据（判定归 B3 人工）:");
    foreach (var p in profs)
        Console.WriteLine($"  PROF @ wall={p.WallMs / 1000.0:F1}s sim={p.SimUs / 1_000_000.0:F1}s scale={p.Scale:F3}");
}
else Console.WriteLine("无 PROF 标记（数据无该段——非错误）");

// ---- TL-04 手动标记（R 键）与自动检测对照 ----
var rstMarks = events.Where(e => e.Kind == "RST").ToList();
Console.WriteLine("\n=== TL-04 手动标记（R 键）与自动检测对照 ===");
if (rstMarks.Count > 0)
{
    Console.WriteLine($"R 键标记 {rstMarks.Count} 处:");
    foreach (var r in rstMarks)
    {
        var near = resets
            .Select(x => (x, Dist: Math.Abs(x.WallMs - r.WallMs)))
            .OrderBy(x => x.Dist)
            .FirstOrDefault();
        string hit = near.x.WallMs == 0 && near.Dist > 3000 ? "（无邻近自动检出——纯位置传送场景）" : $"（邻近自动检出距 {near.Dist / 1000.0:F1}s）";
        Console.WriteLine($"  RST @ wall={r.WallMs / 1000.0:F1}s {hit}");
    }
}
else Console.WriteLine("无 R 键标记（数据无该段——非错误）");

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
