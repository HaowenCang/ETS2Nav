// semaphore-scan v4（调试）：输出静态候选的原始字段 + 玩家坐标，不做过滤
using System.Diagnostics;
using System.IO.MemoryMappedFiles;
using System.Runtime.InteropServices;

(double X, double Z)? playerPos = null;
try
{
    using var mmf = MemoryMappedFile.OpenExisting(@"Local\ETS2NavTelemetry");
    using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);
    playerPos = (view.ReadDouble(0x38), view.ReadDouble(0x48));
    Console.WriteLine($"玩家坐标: ({playerPos.Value.X:F1}, {playerPos.Value.Z:F1})");
}
catch (FileNotFoundException) { Console.WriteLine("玩家坐标不可用（scs-nav-bridge 未加载）"); }

var proc = Process.GetProcessesByName("eurotrucks2").FirstOrDefault();
if (proc is null) { Console.WriteLine("未找到 eurotrucks2"); Console.ReadKey(); return; }
Console.WriteLine($"PID={proc.Id}");

int[] validStates = [0, 1, 2, 4, 8, 32];
int shown = 0;
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

    for (int i = 0; i + 48 <= buf.Length; i += 4)
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
        short cx = BitConverter.ToInt16(buf, i + 12);
        short cy = BitConverter.ToInt16(buf, i + 14);
        double wx1 = px + cx * 512.0, wz1 = pz + cy * 512.0;
        string dist1 = playerPos is { } pp ? $"{(wx1 - pp.X):F0},{(wz1 - pp.Z):F0}" : "-";
        string dist2 = playerPos is { } qq ? $"{(px - qq.X):F0},{(pz - qq.Z):F0}" : "-";
        if (shown++ < 40)
            Console.WriteLine($"0x{((long)mbi.BaseAddress + i):x12} pos=({px:F0},{BitConverter.ToSingle(buf, i + 4):F0},{pz:F0}) cx={cx} cy={cy} type={type} time={time:F1} state={state} d1=({dist1}) d2=({dist2})");
    }
}
Console.WriteLine($"扫描 {scanned / 1024 / 1024} MB，静态候选 {shown}（前 40 条）");
var outPath = Path.Combine(AppContext.BaseDirectory, "semaphore-scan-results.txt");
File.WriteAllLines(outPath, new[] { $"player=({playerPos?.X:F1},{playerPos?.Z:F1})", $"candidates={shown}" });
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
