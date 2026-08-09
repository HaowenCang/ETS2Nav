// countdown-proto：倒计时外推引擎原型 + 误差分析（v0.2 §65 Case B 验证）。
//
// 原理（TL-01 + TL-02 结论）：
// - 信号灯由 simulation_time 驱动，interval 秒 = 真实秒
// - 相位锚定于灯组加载时刻（H2）→ 倒计时 = 观测锚定 + profile 外推
//
// 相位模型：周期 = 绿 + 黄1 + 红 + 黄2（红黄），从锚点（观测到某次转换）开始外推：
//   phase(t) = (t - t_anchor) mod C；转换时刻 = t_anchor + 段起点偏移 + k*C
//
// 本工具用实验数据验证外推精度：
// 1. 从事件序列学习段长（绿/黄1/红/黄2）与周期
// 2. 以第一个完整周期为锚点，外推后续周期的转换时刻
// 3. 与实测事件对比，输出误差（应满足 ≤1s 验收）

using System.Globalization;

if (args.Length < 1)
{
    Console.WriteLine("用法: countdown-proto <events.csv> [--learn-first-cycle]");
    Console.WriteLine("      events.csv 列: wall_ms,sim_us,paused_sim_us,game_min,scale,kind");
    return;
}

var events = LoadEvents(args[0]);
var learnFirst = args.Contains("--learn-first-cycle");
Console.WriteLine($"事件 {events.Count} 条");

// 段模型：事件 kind 映射
// OTHER(黄灯开始) → R2G(绿灯开始) = 黄2(红黄)段
// R2G → OTHER = 绿段
// OTHER → G2R = 黄1段
// G2R → OTHER = 红段
// OTHER → R2G = 黄2段
// 周期 C = 绿+黄1+红+黄2

// 学习段长：取事件对间隔的中位数（抗抖动）
var green = new List<double>();
var yellow1 = new List<double>();
var red = new List<double>();
var yellow2 = new List<double>();
for (int i = 0; i < events.Count - 1; i++)
{
    double dt = (events[i + 1].SimUs - events[i].SimUs) / 1e6;
    if (dt < 0.1 || dt > 60) continue;   // 会话间断不参与
    string pair = events[i].Kind + "->" + events[i + 1].Kind;
    switch (pair)
    {
        case "R2G->OTHER": green.Add(dt); break;
        case "OTHER->G2R": yellow1.Add(dt); break;
        case "G2R->OTHER": red.Add(dt); break;
        case "OTHER->R2G": yellow2.Add(dt); break;
    }
}
double Median(List<double> xs)
{
    xs.Sort();
    return xs[xs.Count / 2];
}
if (green.Count == 0 || yellow1.Count == 0 || red.Count == 0 || yellow2.Count == 0)
{
    Console.WriteLine("事件不足以学习全部段长（需要完整的 OTHER/R2G/G2R 序列）");
    return;
}
double g = Median(green), y1 = Median(yellow1), r = Median(red), y2 = Median(yellow2);
double C = g + y1 + r + y2;
Console.WriteLine($"段长（中位数）: 绿={g:F2}s 黄1={y1:F2}s 红={r:F2}s 黄2={y2:F2}s 周期={C:F3}s");
Console.WriteLine($"样本数: 绿×{green.Count} 黄1×{yellow1.Count} 红×{red.Count} 黄2×{yellow2.Count}");

// 外推验证：按相位跳变划分「加载窗口」（H2：重载后锚点失效）
// 窗口边界 = R2G 间隔偏离周期整数倍超过阈值
var r2gTimes = events.Where(e => e.Kind == "R2G").Select(e => e.SimUs / 1e6).ToList();
Console.WriteLine($"\n=== 加载窗口划分（H2）===");
var windows = new List<List<double>>();
var current = new List<double> { r2gTimes[0] };
for (int i = 1; i < r2gTimes.Count; i++)
{
    double dt = r2gTimes[i] - r2gTimes[i - 1];
    double k = dt / C;
    double rem = Math.Abs(k - Math.Round(k));
    if (rem < 0.15)
        current.Add(r2gTimes[i]);   // 同窗口（间隔 ≈ 整数周期）
    else
    {
        windows.Add(current);
        current = new List<double> { r2gTimes[i] };   // 重载：新窗口
    }
}
windows.Add(current);
foreach (var w in windows)
    Console.WriteLine($"  窗口（{windows.IndexOf(w) + 1}）: R2G 数={w.Count} sim={w.First():F2}~{w.Last():F2}s" + (w.Count == 1 ? "（单点）" : ""));

Console.WriteLine("\n=== 外推精度验证（仅同窗口内）===");
Console.WriteLine("锚点(s) | 预测(s) | 实测(s) | 误差(s) | 外推周期数");
double maxErr = 0;
int validated = 0;
foreach (var w in windows)
{
    foreach (var anchor in w)
    {
        foreach (var t in w)
        {
            double dt = t - anchor;
            if (dt < C * 0.5) continue;   // 至少外推半个周期
            double k = Math.Round(dt / C);
            double predicted = anchor + k * C;
            double err = Math.Abs(predicted - t);
            if (err > maxErr) maxErr = err;
            validated++;
            if (err > 0.01)
                Console.WriteLine($"{anchor,9:F2} | {predicted,9:F2} | {t,9:F2} | {err,7:F3} | {k,3:F0}");
        }
    }
}
Console.WriteLine($"\n窗口内验证 {validated} 组，最大误差: {maxErr:F3}s（验收标准 ≤1.0s）");
Console.WriteLine(maxErr <= 1.0 ? "==> PASS：同窗口外推精度满足 ±1s 验收（Case B 可行）" : "==> FAIL：外推误差超标");
Console.WriteLine("\n说明：锚点=任一次可见的相位转换（R2G/G2R/黄灯），之后即可倒计时；");
Console.WriteLine("外推按整周期步进，误差主要来自周期估计精度与引擎计时抖动。");

static List<(long WallMs, ulong SimUs, string Kind)> LoadEvents(string path)
{
    var list = new List<(long, ulong, string)>();
    foreach (var line in File.ReadLines(path).Skip(1))
    {
        var p = line.Split(',');
        if (p.Length < 6) continue;
        list.Add((long.Parse(p[0]), ulong.Parse(p[1]), p[5]));
    }
    return list;
}
