// semaphore-scan v5：状态机行为验证版
// 决定性特征：真实信号灯在采样窗口内（10s）必然发生 state 转换（RED<->GREEN 等）
//   + time_remaining 递减并在转换时重置。随机内存/零填充不具备此行为。
// 用法：游戏在信号灯路口附近（灯在工作）时执行

using System.Diagnostics;
using System.Runtime.InteropServices;

sealed class Track
{
    public long Addr;
    public int[] States = new int[100];
    public float[] Times = new float[100];
    public int Valid;
    public Track(long a) { Addr = a; }
}

var proc = Process.GetProcessesByName("eurotrucks2").FirstOrDefault();
if (proc is null) { Console.WriteLine("未找到 eurotrucks2 进程"); Console.ReadKey(); return; }
Console.WriteLine($"PID={proc.Id}");

int[] validStates = [0, 1, 2, 4, 8, 32];
var rawCandidates = new List<long>();
long scanned = 0;
var addr = IntPtr.Zero;

// 第一轮：静态候选（排除零填充）
while (Native.VirtualQueryEx(proc.Handle, addr, out var mbi, (uint)Marshal.SizeOf<Native.MEMORY_BASIC_INFORMATION>()) != 0)
{
    addr = (IntPtr)((long)mbi.BaseAddress + (long)mbi.RegionSize);
    if (mbi.State != 0x1000) continue;
    int prot = (int)mbi.Protect;
    if (prot is 0 or 0x100 or 0x200 or 0x300 or 0x400 or 0x500 or 0x600 or 0x700) continue;
    if ((long)mbi.BaseAddress < 0x140000000) continue;
    long region = Math.Min((long)mbi.RegionSize, 8L * 1024 * 1024);
    var buf = new byte[region];
    if (!Native.ReadProcessMemory(proc.Handle, mbi.BaseAddress, buf, buf.Length, out _)) continue;
    scanned += buf.Length;

    for (int i = 0; i + 96 <= buf.Length; i += 4)
    {
        int state = BitConverter.ToInt32(buf, i + 44);
        if (Array.IndexOf(validStates, state) < 0) continue;
        bool zeroSlot = BitConverter.ToInt32(buf, i) == 0 && BitConverter.ToInt32(buf, i + 4) == 0
            && BitConverter.ToInt32(buf, i + 8) == 0 && BitConverter.ToSingle(buf, i + 40) == 0
            && state == 0;
        bool nextZero = BitConverter.ToInt32(buf, i + 48) == 0 && BitConverter.ToInt32(buf, i + 52) == 0
            && BitConverter.ToInt32(buf, i + 56) == 0 && BitConverter.ToSingle(buf, i + 88) == 0
            && BitConverter.ToInt32(buf, i + 92) == 0;
        if (zeroSlot && nextZero) continue;
        float time = BitConverter.ToSingle(buf, i + 40);
        if (time is < -1 or > 300) continue;
        rawCandidates.Add((long)mbi.BaseAddress + i);
    }
}
Console.WriteLine($"静态候选（非零填充）：{rawCandidates.Count} 处（扫描 {scanned / 1024 / 1024} MB）");

// 第二轮：10 秒跟踪（100ms/帧），验证状态机行为
const int frames = 100;
var active = rawCandidates.Select(a => new Track(a)).ToList();
var sw = Stopwatch.StartNew();
for (int f = 0; f < frames; f++)
{
    for (int k = 0; k < active.Count; k++)
    {
        var one = new byte[48];
        if (Native.ReadProcessMemory(proc.Handle, (IntPtr)active[k].Addr, one, 48, out _))
        {
            active[k].States[f] = BitConverter.ToInt32(one, 44);
            active[k].Times[f] = BitConverter.ToSingle(one, 40);
            active[k].Valid++;
        }
    }
    if (f < frames - 1) Thread.Sleep(100);
}
Console.WriteLine($"跟踪完成（{sw.Elapsed.TotalSeconds:F1}s）");

// 判定：state 变化 ≥1 次 且 time 存在递减段（>1s）
var confirmed = new List<(long Addr, int Transitions, float MaxDrop)>();
foreach (var c in active)
{
    if (c.Valid < 30) continue;
    int transitions = 0;
    for (int f = 1; f < frames; f++)
        if (c.States[f] != 0 && c.States[f - 1] != 0 && c.States[f] != c.States[f - 1])
            transitions++;
    float maxDrop = 0;
    for (int f = 1; f < frames; f++)
    {
        float a = c.Times[f - 1], b = c.Times[f];
        if (a > 0.5f && b >= 0 && a - b > maxDrop) maxDrop = a - b;
    }
    if (transitions >= 1 || maxDrop > 2.0f)
        confirmed.Add((c.Addr, transitions, maxDrop));
}
Console.WriteLine($"状态机验证通过：{confirmed.Count} 处");
foreach (var (a, tr, md) in confirmed.Take(30))
    Console.WriteLine($"  0x{a:x12}  转换×{tr}  最大递减 {md:F1}s");

var outPath = Path.Combine(AppContext.BaseDirectory, "semaphore-scan-results.txt");
File.WriteAllLines(outPath, confirmed.Select(c => $"0x{c.Addr:x12} t={c.Transitions} drop={c.MaxDrop:F1}"));
Console.WriteLine($"结果已保存：{outPath}（{confirmed.Count} 处）");
Console.WriteLine("按任意键退出...");
Console.ReadKey();

static class Native
{
    [StructLayout(LayoutKind.Sequential)]
    public struct MEMORY_BASIC_INFORMATION
    {
        public IntPtr BaseAddress; public IntPtr AllocationBase; public uint AllocationProtect;
        public IntPtr RegionSize; public uint State; public uint Protect; public uint Type;
    }
    [DllImport("kernel32.dll")]
    public static extern int VirtualQueryEx(IntPtr hProcess, IntPtr lpAddress, out MEMORY_BASIC_INFORMATION lpBuffer, uint dwLength);
    [DllImport("kernel32.dll")]
    public static extern bool ReadProcessMemory(IntPtr hProcess, IntPtr lpBaseAddress, byte[] lpBuffer, int dwSize, out IntPtr lpNumberOfBytesRead);
}
