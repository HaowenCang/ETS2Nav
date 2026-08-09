// semaphore-scan：游戏内存信号灯结构扫描器（自研内存读取可行性侦察）
// 特征（依据 ETS2LA 公开布局，很可能是游戏内部结构镜像）：
//   state 枚举 ∈ {0,1,2,4,8,32}、time_remaining float ∈ [0,300]、
//   position 世界坐标（±500000）、type ∈ {1,2}、48 字节/灯连续排列
// 结果同时输出到控制台与 semaphore-scan-results.txt（防窗口关闭丢失）
// 用法：游戏在信号灯路口附近运行时执行

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
var candidates = new List<(long Addr, int Count)>();
long scanned = 0;
var addr = IntPtr.Zero;
while (Native.VirtualQueryEx(proc.Handle, addr, out var mbi, (uint)Marshal.SizeOf<Native.MEMORY_BASIC_INFORMATION>()) != 0)
{
    addr = (IntPtr)((long)mbi.BaseAddress + (long)mbi.RegionSize);
    if (mbi.State != 0x1000) continue;
    int prot = (int)mbi.Protect;
    if (prot is 0 or 0x100 or 0x200 or 0x300 or 0x400 or 0x500 or 0x600 or 0x700) continue;
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
        {
            long abs = (long)mbi.BaseAddress + i;
            candidates.Add((abs, 1));
        }
    }
}

Console.WriteLine($"扫描 {scanned / 1024 / 1024} MB，候选 {candidates.Count} 处");
foreach (var c in candidates.Take(50))
    Console.WriteLine($"  0x{c.Addr:x12}");

var outPath = Path.Combine(AppContext.BaseDirectory, "semaphore-scan-results.txt");
File.WriteAllLines(outPath, candidates.Take(100).Select(c => $"0x{c.Addr:x12}"));
Console.WriteLine($"结果已保存：{outPath}");
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
