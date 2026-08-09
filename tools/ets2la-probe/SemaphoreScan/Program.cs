// semaphore-scan v2：两轮动态验证的信号灯结构扫描器
// 第一轮：静态特征匹配候选（state 枚举 + time float + 坐标 + 48B/灯）
// 第二轮（500ms 后）：候选的 time_remaining 必须递减（Δ≈0.5-2s）→ 真信号灯
// 排除模块映像区（低地址）减少误报
// 用法：游戏在信号灯路口附近运行时执行；结果写入 semaphore-scan-results.txt

using System.Diagnostics;
using System.Runtime.InteropServices;

var proc = Process.GetProcessesByName("eurotrucks2").FirstOrDefault();
if (proc is null)
{
    Console.WriteLine("未找到 eurotrucks2 进程——请先启动游戏并进入驾驶界面");
    Console.WriteLine("按任意键退出...");
    Console.ReadKey();
    return;
}
Console.WriteLine($"PID={proc.Id}");

int[] validStates = [0, 1, 2, 4, 8, 32];
var candidates = new List<(long Addr, float Time)>();
long scanned = 0;
var addr = IntPtr.Zero;

// 第一轮：静态特征
while (Native.VirtualQueryEx(proc.Handle, addr, out var mbi, (uint)Marshal.SizeOf<Native.MEMORY_BASIC_INFORMATION>()) != 0)
{
    addr = (IntPtr)((long)mbi.BaseAddress + (long)mbi.RegionSize);
    if (mbi.State != 0x1000) continue;
    int prot = (int)mbi.Protect;
    if (prot is 0 or 0x100 or 0x200 or 0x300 or 0x400 or 0x500 or 0x600 or 0x700) continue;
    // 跳过模块映像区（基址附近，通常是静态数据误报源）
    if ((long)mbi.BaseAddress < 0x140000000) continue;
    long region = Math.Min((long)mbi.RegionSize, 8L * 1024 * 1024);
    var buf = new byte[region];
    if (!Native.ReadProcessMemory(proc.Handle, mbi.BaseAddress, buf, buf.Length, out _)) continue;
    scanned += buf.Length;

    for (int i = 0; i + 96 <= buf.Length; i += 4)
    {
        int state = BitConverter.ToInt32(buf, i + 44);
        if (Array.IndexOf(validStates, state) < 0) continue;
        int type = BitConverter.ToInt32(buf, i + 36);
        if (type is not (1 or 2)) continue;
        float time = BitConverter.ToSingle(buf, i + 40);
        if (time is < -1 or > 300) continue;
        float px = BitConverter.ToSingle(buf, i);
        float pz = BitConverter.ToSingle(buf, i + 8);
        if (Math.Abs(px) > 500000 || Math.Abs(pz) > 500000) continue;
        int ns = BitConverter.ToInt32(buf, i + 48 + 44);
        bool nextOk = ns == 0 || Array.IndexOf(validStates, ns) >= 0;
        if (nextOk)
            candidates.Add(((long)mbi.BaseAddress + i, time));
    }
}
Console.WriteLine($"第一轮：扫描 {scanned / 1024 / 1024} MB，静态候选 {candidates.Count} 处");

// 第二轮：动态验证（time_remaining 递减）
if (candidates.Count > 0)
{
    Console.WriteLine("等待 800ms 后验证递减...");
    Thread.Sleep(800);
    var confirmed = new List<(long Addr, float T0, float T1)>();
    foreach (var (caddr, t0) in candidates)
    {
        // 读取同一地址当前 time（跨页处理：候选在缓冲区中的偏移未知，直接按地址读 48 字节）
        var one = new byte[48];
        if (!Native.ReadProcessMemory(proc.Handle, (IntPtr)caddr, one, 48, out _)) continue;
        float t1 = BitConverter.ToSingle(one, 40);
        float dt = t0 - t1;
        // 递减 0.2~2.5s（800ms 间隔）→ 真实倒计时；红绿灯转换瞬间 t1 可能重置（周期跳变），容差放宽
        if (dt is > 0.05f and < 5.0f)
            confirmed.Add((caddr, t0, t1));
    }
    Console.WriteLine($"动态验证通过：{confirmed.Count} 处");
    foreach (var (caddr, t0, t1) in confirmed.Take(50))
        Console.WriteLine($"  0x{caddr:x12}  time {t0:F1}s -> {t1:F1}s");

    var outPath = Path.Combine(AppContext.BaseDirectory, "semaphore-scan-results.txt");
    File.WriteAllLines(outPath, confirmed.Select(c => $"0x{c.Addr:x12}"));
    Console.WriteLine($"结果已保存：{outPath}（{confirmed.Count} 处）");
}
else
{
    Console.WriteLine("无候选。请确认：游戏在驾驶界面、信号灯路口附近、灯在工作。");
    var outPath = Path.Combine(AppContext.BaseDirectory, "semaphore-scan-results.txt");
    File.WriteAllLines(outPath, Array.Empty<string>());
}
Console.WriteLine("按任意键退出...");
Console.ReadKey();

static class Native
{
    [StructLayout(LayoutKind.Sequential)]
    public struct MEMORY_BASIC_INFORMATION
    {
        public IntPtr BaseAddress;
        public IntPtr AllocationBase;
        public uint AllocationProtect;
        public IntPtr RegionSize;
        public uint State;
        public uint Protect;
        public uint Type;
    }
    [DllImport("kernel32.dll")]
    public static extern int VirtualQueryEx(IntPtr hProcess, IntPtr lpAddress, out MEMORY_BASIC_INFORMATION lpBuffer, uint dwLength);
    [DllImport("kernel32.dll")]
    public static extern bool ReadProcessMemory(IntPtr hProcess, IntPtr lpBaseAddress, byte[] lpBuffer, int dwSize, out IntPtr lpNumberOfBytesRead);
}
