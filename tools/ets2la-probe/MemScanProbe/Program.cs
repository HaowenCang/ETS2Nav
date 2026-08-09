// memscan-probe v2：float 值搜索 + 地址上下文 dump（ETS2LA 数据反查用）
// 用法1：memscan-probe eurotrucks2 --float <值>          → 搜索 float 的 4 字节模式
// 用法2：memscan-probe eurotrucks2 --dump <hex地址> [字节数]  → dump 指定地址上下文
using System.Diagnostics;
using System.Runtime.InteropServices;

if (args.Length < 2) { Console.WriteLine("用法: memscan-probe eurotrucks2 --float <值> | --dump <hex地址> [len]"); return; }

var proc = Process.GetProcessesByName("eurotrucks2").FirstOrDefault();
if (proc is null) { Console.WriteLine("未找到 eurotrucks2"); return; }

if (args[1] == "--dump")
{
    long baseAddr = Convert.ToInt64(args[2], 16);
    int len = args.Length > 3 ? int.Parse(args[3]) : 256;
    var buf = new byte[len];
    if (!Native.ReadProcessMemory(proc.Handle, (IntPtr)baseAddr, buf, len, out _))
    { Console.WriteLine("读取失败"); return; }
    for (int row = 0; row < len; row += 16)
    {
        Console.Write($"0x{baseAddr + row:x12}: ");
        for (int k = 0; k < 16 && row + k < len; k++) Console.Write($"{buf[row + k]:02x} ");
        Console.Write(" | ");
        for (int k = 0; k < 16 && row + k < len; k++)
        {
            byte b = buf[row + k];
            Console.Write(b >= 0x20 && b < 0x7f ? (char)b : '.');
        }
        // 标注 float 候选
        Console.Write("  ");
        for (int k = 0; k + 4 <= 16 && row + k + 4 <= len; k += 4)
        {
            float f = BitConverter.ToSingle(buf, row + k);
            if (!float.IsNaN(f) && !float.IsInfinity(f) && Math.Abs(f) < 100000)
                Console.Write($"[{f:F2}] ");
        }
        Console.WriteLine();
    }
    return;
}

if (args[1] == "--float")
{
    float target = float.Parse(args[2]);
    var needle = BitConverter.GetBytes(target);
    Console.WriteLine($"搜索 float {target} = {BitConverter.ToUInt32(needle):x8}，进程 {proc.Id}");
    var hits = new List<long>();
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
        for (int i = 0; i + 4 <= buf.Length; i++)
        {
            if (buf[i] == needle[0] && buf[i + 1] == needle[1] && buf[i + 2] == needle[2] && buf[i + 3] == needle[3])
            {
                long abs = (long)mbi.BaseAddress + i;
                hits.Add(abs);
                if (hits.Count <= 30) Console.WriteLine($"  0x{abs:x12}");
            }
        }
    }
    Console.WriteLine($"扫描 {scanned / 1024 / 1024} MB，命中 {hits.Count} 处");
    var outPath = Path.Combine(AppContext.BaseDirectory, "float-search-results.txt");
    File.WriteAllLines(outPath, hits.Take(500).Select(h => $"0x{h:x12}"));
    Console.WriteLine($"已保存：{outPath}");
    return;
}

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
