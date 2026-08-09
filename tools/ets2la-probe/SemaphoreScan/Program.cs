// semaphore-scan v7：紧凑模式扫描 + ETS2LA 联合验证（决定性）
// 模式：{state:int ∈ {1,2,4,8,32}} 与相邻 {float ∈ [0,60]}（两种顺序）
// 验证：候选的 state/time 与 ETS2LA 共享内存当前值对照（联合匹配 → 真实结构）
using System.Diagnostics;
using System.IO.MemoryMappedFiles;
using System.Runtime.InteropServices;

// 1) ETS2LA 当前信号灯（ground truth）
var ets2la = new List<(int State, float Time, float X, float Z)>();
try
{
    using var mmf = MemoryMappedFile.OpenExisting(@"Local\ETS2LASemaphore");
    using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);
    var buf = new byte[40 * 48];
    view.ReadArray(0, buf, 0, buf.Length);
    for (int i = 0; i < 40; i++)
    {
        int o = i * 48;
        int type = BitConverter.ToInt32(buf, o + 32);
        int state = BitConverter.ToInt32(buf, o + 40);
        float time = BitConverter.ToSingle(buf, o + 36);
        if (type == 1 && time > 0 && time < 60)
            ets2la.Add((state, time, BitConverter.ToSingle(buf, o), BitConverter.ToSingle(buf, o + 8)));
    }
    Console.WriteLine($"ETS2LA 活动灯组 {ets2la.Count} 组");
    foreach (var e in ets2la.Take(5)) Console.WriteLine($"  state={e.State} time={e.Time:F1}s pos=({e.X:F0},{e.Z:F0})");
}
catch (FileNotFoundException) { Console.WriteLine("ETS2LA 共享内存不可用——联合验证失效"); }

// 2) 游戏内存紧凑模式扫描
var proc = Process.GetProcessesByName("eurotrucks2").FirstOrDefault();
if (proc is null) { Console.WriteLine("未找到 eurotrucks2"); return; }

int[] validStates = [1, 2, 4, 8, 32];
var candidates = new List<(long Addr, int State, float Time)>();
long scanned = 0;
var addr = IntPtr.Zero;
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

    for (int i = 0; i + 8 <= buf.Length; i += 4)
    {
        // 模式1：int(state) + float(time)
        int s = BitConverter.ToInt32(buf, i);
        float t = BitConverter.ToSingle(buf, i + 4);
        if (Array.IndexOf(validStates, s) >= 0 && t >= 0 && t <= 60)
            candidates.Add(((long)mbi.BaseAddress + i, s, t));
        // 模式2：float(time) + int(state)
        float t2 = BitConverter.ToSingle(buf, i);
        int s2 = BitConverter.ToInt32(buf, i + 4);
        if (Array.IndexOf(validStates, s2) >= 0 && t2 >= 0 && t2 <= 60)
            candidates.Add(((long)mbi.BaseAddress + i, s2, t2));
    }
}
Console.WriteLine($"紧凑模式候选：{candidates.Count} 处（扫描 {scanned / 1024 / 1024} MB）");

// 3) 联合验证：候选与 ETS2LA 值匹配
int matched = 0;
foreach (var c in candidates.Take(200000))
{
    foreach (var e in ets2la)
    {
        if (c.State == e.State && Math.Abs(c.Time - e.Time) < 1.0)
        {
            matched++;
            if (matched <= 20)
                Console.WriteLine($"匹配! 0x{c.Addr:x12} state={c.State} time={c.Time:F1}s（ETS2LA: {e.Time:F1}s）");
            break;
        }
    }
}
Console.WriteLine($"联合匹配：{matched} 处");
var outPath = Path.Combine(AppContext.BaseDirectory, "semaphore-scan-results.txt");
File.WriteAllLines(outPath, candidates.Where(c => ets2la.Any(e => c.State == e.State && Math.Abs(c.Time - e.Time) < 1.0))
    .Select(c => $"0x{c.Addr:x12} state={c.State} time={c.Time:F1}").Take(100));
Console.WriteLine($"结果已保存：{outPath}");
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
