// P1-09 speed-validator：读 scs-nav-bridge 共享内存（Local\ETS2NavTelemetry），
// 记录（世界坐标, telemetry speed_limit）轨迹；离线对照 map 预测限速，统计一致率。
// 用法：SpeedValidator --install <游戏根> [--region germany] [--duration 300] [--out trace.csv]
// 游戏运行时执行（需要 ETS2 + scs-nav-bridge 插件）。
using System.Runtime.InteropServices;
using System.Text;
using ScsResource;
using ScsSector;
using ScsDefinitions;
using ScsMapModel;

var cmdArgs = Environment.GetCommandLineArgs().Skip(1).ToArray();
string? Arg(string k) { for (int i = 0; i < cmdArgs.Length - 1; i++) if (cmdArgs[i] == k) return cmdArgs[i + 1]; return null; }
var installRoot = Arg("--install") ?? @"E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2";
var region = Arg("--region") ?? "germany";
var durationSec = int.Parse(Arg("--duration") ?? "300");
var outPath = Arg("--out") ?? "speed-trace.csv";
var trackOnly = cmdArgs.Contains("--track");   // 仅记录轨迹（不加载 map——快速）

Console.WriteLine($"speed-validator：读取 {installRoot}，区域 {region}，时长 {durationSec}s");

// 加载 map（限速模型）
SpeedModel? speeds = null;
double[]? xs = null, zs = null, limits = null;
if (!trackOnly)
{
    var install = GameInstall.Detect(installRoot);
    using var overlay = install.BuildOverlay();
    var names = overlay.Enumerate("/map/europe")
        .Where(p => p.EndsWith(".base") && p.Contains("/sec+"))
        .Select(p => Path.GetFileNameWithoutExtension(p))
        .Where(n => RegionMatch(n, region))
        .OrderBy(n => n).ToList();
    var sectors = new List<SectorFile>();
    foreach (var name in names)
    {
        using var s = overlay.Open($"/map/europe/{name}.base");
        sectors.Add(SectorFile.Read(s, name));
    }
    var defs = new DefinitionResolver(overlay);
    speeds = new SpeedModel(defs, sectors);
    // 构建道路限速空间索引（road 段列表）
    var roadSegs = new List<(double X0, double Z0, double X1, double Z1, int Limit)>();
    foreach (var sec in sectors)
    {
        var nodePos = sec.Nodes.ToDictionary(n => n.Uid);
        foreach (var r in sec.Roads)
        {
            if (!nodePos.TryGetValue(r.Node0, out var a) || !nodePos.TryGetValue(r.Node1, out var b)) continue;
            var sc = r.RightTrafficRule.Length > 0 ? r.RightTrafficRule : r.LeftTrafficRule;
            if (sc.Length == 0)
            {
                var look = defs.GetRoadLook(r.RoadLook);
                var lane = look?.LanesRight.FirstOrDefault() ?? look?.LanesLeft.FirstOrDefault();
                if (lane != null) sc = defs.GetTrafficLane(lane)?.SpeedClass ?? "";
            }
            if (sc.Length == 0) continue;
            var limit = speeds.GetSpeedLimit(a.X, a.Z, sc, r.IsCityRoad);
            roadSegs.Add((a.X, a.Z, b.X, b.Z, limit));
        }
    }
    xs = roadSegs.Select(s => s.X0).ToArray(); zs = roadSegs.Select(s => s.Z0).ToArray();
    limits = roadSegs.Select(s => (double)s.Limit).ToArray();
    Console.WriteLine($"map 道路段 {roadSegs.Count}（含限速）");
}

// 共享内存读取
var h = OpenFileMappingA(0x0004 /*FILE_MAP_READ*/, false, "Local\\ETS2NavTelemetry");
if (h == IntPtr.Zero)
{
    Console.WriteLine("共享内存未找到——请先启动游戏（含 scs-nav-bridge 插件）");
    return;
}
var ptr = MapViewOfFile(h, 0x0004, 0, 0, 0);
if (ptr == IntPtr.Zero) { Console.WriteLine("MapViewOfFile 失败"); return; }
try
{
    // 结构偏移：placement 在 telemetry_state_t 中——用偏移读取关键字段
    // （与 nav-bridge 布局一致：placement 字段偏移需匹配——此处用已知布局扫描）
    // placement 世界坐标 + speed + speed_limit
    var sb = new StringBuilder();
    sb.AppendLine("time_ms,x,z,speed_kmh,telemetry_limit_kmh,map_limit_kmh");
    var sw = System.Diagnostics.Stopwatch.StartNew();
    int samples = 0, matches = 0, near = 0;
    long lastSample = 0;
    while (sw.ElapsedMilliseconds < durationSec * 1000L)
    {
        // 结构偏移（nav-bridge telemetry_state_t，pack(1)）：
        // sequence 0x00(4) + layout 0x04(4) + sync 0x08(4) + sim 0x0C(8) + paused 0x14(8) +
        // render 0x1C(8) + elapsed 0x24(8) + game_min 0x2C(4) + scale 0x30(4) + rest 0x34(4)
        // = placement @0x38（pos 3×double + quat 4×float = 40B）→ speed @0x60 → speed_limit @0x64
        long t = sw.ElapsedMilliseconds;
        double x = ReadDouble(ptr, 0x38);
        double y = ReadDouble(ptr, 0x40);
        double z = ReadDouble(ptr, 0x48);
        float speed = ReadFloat(ptr, 0x60);
        float telLimit = ReadFloat(ptr, 0x64);
        if (telLimit > 0 && t - lastSample >= 1000)   // 每秒采样
        {
            lastSample = t;
            int mapLimit = 0;
            if (speeds != null)
                mapLimit = NearestLimit(x, z, xs!, zs!, limits!);
            double telKmh = telLimit * 3.6;
            sb.AppendLine($"{t},{x:F1},{z:F1},{speed * 3.6:F1},{telKmh:F0},{mapLimit}");
            samples++;
            if (mapLimit > 0)
            {
                if (Math.Abs(mapLimit - telKmh) <= 5) matches++;
                if (Math.Abs(mapLimit - telKmh) <= 20) near++;
            }
        }
        Thread.Sleep(100);
    }
    File.WriteAllText(outPath, sb.ToString());
    Console.WriteLine($"采样 {samples} 条 → {outPath}");
    if (samples > 0)
    {
        Console.WriteLine($"一致率（±5km/h）：{matches}/{samples}（{100.0 * matches / samples:F1}%）");
        Console.WriteLine($"近似一致率（±20km/h）：{near}/{samples}（{100.0 * near / samples:F1}%）");
    }
}
finally
{
    UnmapViewOfFile(ptr);
    CloseHandle(h);
}

static int NearestLimit(double x, double z, double[] xs, double[] zs, double[] limits)
{
    double best = double.MaxValue;
    int bestLimit = 0;
    for (int i = 0; i < xs.Length; i++)
    {
        // 点到线段距离（近似：端点距离取小）
        double d = Math.Min(Math.Sqrt((x - xs[i]) * (x - xs[i]) + (z - zs[i]) * (z - zs[i])), 1e9);
        if (d < best) { best = d; bestLimit = (int)limits[i]; }
    }
    return best < 30 ? bestLimit : 0;   // 30m 内才匹配
}

static bool RegionMatch(string name, string region)
{
    var m = System.Text.RegularExpressions.Regex.Match(name, @"^sec\+(\d+)([+-])(\d+)$");
    if (!m.Success) return false;
    int x = int.Parse(m.Groups[1].Value);
    int y = int.Parse(m.Groups[3].Value) * (m.Groups[2].Value == "-" ? -1 : 1);
    return region switch { "germany" => x >= -1 && x <= 3 && y >= -6 && y <= 3, _ => true };
}

[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Ansi)]
static extern IntPtr OpenFileMappingA(uint dwDesiredAccess, bool bInheritHandle, string lpName);
[DllImport("kernel32.dll", SetLastError = true)]
static extern IntPtr MapViewOfFile(IntPtr hFileMappingObject, uint dwDesiredAccess, uint dwFileOffsetHigh, uint dwFileOffsetLow, nuint dwNumberOfBytesToMap);
[DllImport("kernel32.dll")]
static extern bool UnmapViewOfFile(IntPtr lpBaseAddress);
[DllImport("kernel32.dll")]
static extern bool CloseHandle(IntPtr hObject);

static double ReadDouble(IntPtr p, long offset) => Marshal.ReadInt64(p, (int)offset) switch
{
    var v => BitConverter.Int64BitsToDouble(v),
};
static float ReadFloat(IntPtr p, long offset) => BitConverter.Int32BitsToSingle(Marshal.ReadInt32(p, (int)offset));
