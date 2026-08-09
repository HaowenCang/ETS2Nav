// semaphore-scan v3：空间过滤版信号灯结构扫描器
// 决定性特征：信号灯世界坐标 ≈ 玩家坐标（信号灯在路口，玩家在信号灯附近）
//   世界坐标 = position + (cx, 0, cy) * 512（ETS2LA GetWorldCoordinates 语义）
// 过滤链：静态特征 → 玩家距离（<1500m）→ 动态验证（time_remaining 递减）
// 玩家坐标从 Local\ETS2NavTelemetry（我们自己的插件）读取
// 用法：游戏在信号灯路口附近运行时执行

using System.Diagnostics;
using System.IO.MemoryMappedFiles;
using System.Runtime.InteropServices;

// 读取玩家坐标（scs-nav-bridge 共享内存）
(double X, double Z)? playerPos = null;
try
{
    using var mmf = MemoryMappedFile.OpenExisting("Local\\ETS2NavTelemetry");
    using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);
    playerPos = (view.ReadDouble(0x38), view.ReadDouble(0x48));
    Console.WriteLine($"玩家坐标: ({playerPos.Value.X:F1}, {playerPos.Value.Z:F1})");
}
catch (FileNotFoundException)
{
    Console.WriteLine("未找到 ETS2NavTelemetry（scs-nav-bridge 插件未加载？）——空间过滤不可用");
}

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
var candidates = new List<(long Addr, double X, double Z, float Time)>();
long scanned = 0;
var addr = IntPtr.Zero;

// 第一轮：静态特征 + 空间过滤
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
        int type = BitConverter.ToInt32(buf, i + 36);
        if (type is not (1 or 2)) continue;
        float time = BitConverter.ToSingle(buf, i + 40);
        if (time is < -1 or > 300) continue;
        // 世界坐标 = position + (cx, 0, cy)*512
        float px = BitConverter.ToSingle(buf, i);
        float pz = BitConverter.ToSingle(buf, i + 8);
        short cx = BitConverter.ToInt16(buf, i + 12);
        short cy = BitConverter.ToInt16(buf, i + 14);
        double wx = px + cx * 512.0;
        double wz = pz + cy * 512.0;
        if (Math.Abs(wx) > 500000 || Math.Abs(wz) > 500000) continue;
        // 空间过滤：玩家坐标附近 1500m
        if (playerPos is { } pp)
        {
            double dx = wx - pp.X, dz = wz - pp.Z;
            if (dx * dx + dz * dz > 1500.0 * 1500.0) continue;
        }
        int ns = BitConverter.ToInt32(buf, i + 48 + 44);
        if (ns != 0 && Array.IndexOf(validStates, ns) < 0) continue;
        candidates.Add(((long)mbi.BaseAddress + i, wx, wz, time));
    }
}
Console.WriteLine($"第一轮：静态+空间过滤后候选 {candidates.Count} 处");

// 第二轮：动态验证（time_remaining 递减）
var confirmed = new List<(long Addr, double X, double Z, float T0, float T1)>();
if (candidates.Count > 0)
{
    Console.WriteLine("等待 800ms 后验证递减...");
    Thread.Sleep(800);
    foreach (var (caddr, wx, wz, t0) in candidates)
    {
        var one = new byte[48];
        if (!Native.ReadProcessMemory(proc.Handle, (IntPtr)caddr, one, 48, out _)) continue;
        float t1 = BitConverter.ToSingle(one, 40);
        float dt = t0 - t1;
        if (dt is > 0.05f and < 5.0f)
            confirmed.Add((caddr, wx, wz, t0, t1));
    }
    Console.WriteLine($"动态验证通过：{confirmed.Count} 处");
    foreach (var (caddr, wx, wz, t0, t1) in confirmed.Take(50))
        Console.WriteLine($"  0x{caddr:x12}  世界({wx:F0},{wz:F0})  time {t0:F1}s->{t1:F1}s");
}

var outPath = Path.Combine(AppContext.BaseDirectory, "semaphore-scan-results.txt");
File.WriteAllLines(outPath, confirmed.Select(c => $"0x{c.Addr:x12}  ({c.X:F0},{c.Z:F0})  t={c.T0:F1}->{c.T1:F1}"));
Console.WriteLine($"结果已保存：{outPath}（{confirmed.Count} 处）");
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
