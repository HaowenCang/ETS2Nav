// signal-lab：P0-B 红绿灯时钟域/相位锚定实验仪器（v0.2 §29-30、§74）。
//
// 功能：
// 1) 高频采样（默认 100 Hz）：Windows QPC（真实秒）+ 共享内存时钟
//    （simulation_time / paused_simulation_time / render_time / game.time / scale）
// 2) 按键标记信号灯相位转换事件：
//    F9  未分类转换（信号灯状态变化瞬间）
//    1   红灯→绿灯
//    2   绿灯→红灯
//    3   其他转换（黄灯等）
//    S   开始/暂停采样
//    Q   退出
// 3) 输出 CSV：
//    samples_<会话>.csv：时钟轨迹（分析 Δt_signal 与各时钟 Δt 的比例 → clock domain）
//    events_<会话>.csv：相位转换事件（TL-02 相位锚定分析）
//
// 用法：游戏运行 + 插件加载后执行；在信号灯路口观察灯组，灯变色的瞬间按键。
// 一个"会话"对应同一路口的连续观测（S 开始、再按 S 结束）。

using System.Diagnostics;
using System.IO.MemoryMappedFiles;
using System.Text;

const string MapName = "Local\\ETS2NavTelemetry";
const uint LayoutVersion = 1;
const int SampleIntervalMs = 10;      // 100 Hz
const string OutDir = "signal-lab-data";

Console.WriteLine("signal-lab：P0-B 时钟域实验仪器（Ctrl+C/按 Q 退出）");
Console.WriteLine("按键：F9=未分类转换  1=红→绿  2=绿→红  3=其他   R=疑似重置/快速旅行  P=特殊profile路口  S=开始/结束会话  Q=退出");
Console.WriteLine("操作：接近信号灯路口后按 S 开始，灯组变色的瞬间按键标记，驶离后按 S 结束。");

Directory.CreateDirectory(OutDir);
var sessionStart = DateTime.Now.ToString("yyyyMMdd-HHmmss");

using var mmf = MemoryMappedFile.OpenExisting(MapName);
using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);

using var samplesWriter = new StreamWriter(Path.Combine(OutDir, $"samples_{sessionStart}.csv"));
using var eventsWriter = new StreamWriter(Path.Combine(OutDir, $"events_{sessionStart}.csv"));
samplesWriter.WriteLine("wall_ms,sim_us,paused_sim_us,render_us,game_min,scale,speed_kph");
eventsWriter.WriteLine("wall_ms,sim_us,paused_sim_us,game_min,scale,kind");

var sw = Stopwatch.StartNew();
bool sampling = false;
var stop = false;

// 按键监听线程（非阻塞）
var keyThread = new Thread(() =>
{
    while (!stop)
    {
        if (Console.KeyAvailable)
        {
            var key = Console.ReadKey(true);
            switch (key.Key)
            {
                case ConsoleKey.F9: LogEvent("F9"); break;
                case ConsoleKey.D1: LogEvent("R2G"); break;
                case ConsoleKey.D2: LogEvent("G2R"); break;
                case ConsoleKey.D3: LogEvent("OTHER"); break;
                case ConsoleKey.R: LogEvent("RST"); break;   // TL-04：读档/快速旅行疑似重置
                case ConsoleKey.P: LogEvent("PROF"); break;  // TL-05：特殊 profile 路口（sleep_time/blockable）
                case ConsoleKey.S:
                    sampling = !sampling;
                    Console.WriteLine(sampling ? "[会话开始]" : "[会话结束]");
                    break;
                case ConsoleKey.Q: stop = true; break;
            }
        }
        Thread.Sleep(10);
    }
});
keyThread.IsBackground = true;
keyThread.Start();

void LogEvent(string kind)
{
    long wall = sw.ElapsedMilliseconds;
    var (sim, paused, gameMin, scale) = ReadClocks();
    eventsWriter.WriteLine($"{wall},{sim},{paused},{gameMin},{scale:F3},{kind}");
    eventsWriter.Flush();
    Console.WriteLine($"[{kind}] t={wall}ms sim={sim / 1_000_000.0:F3}s scale={scale:F3}");
}

(ulong Sim, ulong Paused, uint GameMin, float Scale) ReadClocks()
{
    return (view.ReadUInt64(0x0C), view.ReadUInt64(0x14), view.ReadUInt32(0x2C), view.ReadSingle(0x30));
}

while (!stop)
{
    if (sampling)
    {
        long wall = sw.ElapsedMilliseconds;
        var (sim, paused, gameMin, scale) = ReadClocks();
        float speed = view.ReadSingle(0x60);
        samplesWriter.WriteLine($"{wall},{sim},{paused},{view.ReadUInt64(0x1C)},{gameMin},{scale:F3},{speed * 3.6:F1}");
        if (wall % 5000 < SampleIntervalMs)
            Console.WriteLine($"  wall={wall / 1000.0:F1}s sim={sim / 1_000_000.0:F3}s paused={paused / 1_000_000.0:F3}s game={gameMin}min scale={scale:F3} speed={speed * 3.6:F0}km/h");
    }
    Thread.Sleep(SampleIntervalMs);
}

samplesWriter.Flush();
eventsWriter.Flush();
Console.WriteLine($"已保存：{Path.Combine(OutDir, $"samples_{sessionStart}.csv")} / events_{sessionStart}.csv");
