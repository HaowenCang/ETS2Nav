// mirror-scan：镜像对比法反查原生灯数组（ETS2LA 在场时运行）
// 输入：ETS2NavSemaphore 已知灯 (time, pos) → 全内存找 time 匹配 + pos 窗口匹配的 float
// 输出：候选地址 + 256B dump
using System.Diagnostics;
using System.IO.MemoryMappedFiles;
using System.Runtime.InteropServices;

class Program
{    const uint MEM_COMMIT = 0x1000;
    const uint PAGE_NOACCESS = 0x01, PAGE_GUARD = 0x100;

    [DllImport("kernel32.dll")]
    static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32.dll")]
    static extern bool VirtualQueryEx(IntPtr h, IntPtr addr, out MEMORY_BASIC_INFORMATION mbi, IntPtr len);
    [DllImport("kernel32.dll")]
    static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, byte[] buf, IntPtr size, out IntPtr read);
    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr h);

    [StructLayout(LayoutKind.Sequential)]
    struct MEMORY_BASIC_INFORMATION
    {
        public IntPtr BaseAddress, AllocationBase;
        public uint AllocationProtect, __alignment1;
        public IntPtr RegionSize;
        public uint State, Protect, Type, __alignment2;
    }

    static int Main()
    {
        var p = Process.GetProcessesByName("eurotrucks2").FirstOrDefault();
        if (p == null) { Console.WriteLine("eurotrucks2 未运行"); return 1; }
        Console.WriteLine($"进程 {p.Id}");
        var proc = OpenProcess(0x0010 | 0x0400 | 0x0008, false, (uint)p.Id);

        var known = new List<(float Time, float X, float Z)>();
        try
        {
            using var mmf = MemoryMappedFile.OpenExisting(@"Local\ETS2NavSemaphore");
            using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);
            uint count = view.ReadUInt32(12);
            var buf = new byte[(int)count * 48];
            view.ReadArray(16, buf, 0, buf.Length);
            for (int i = 0; i < count; i++)
            {
                int o = i * 48;
                float time = BitConverter.ToSingle(buf, o + 36);
                if (time > 0.5f && time < 59f)
                    known.Add((time, BitConverter.ToSingle(buf, o), BitConverter.ToSingle(buf, o + 8)));
            }
            Console.WriteLine($"已知灯 {known.Count} 盏");
            foreach (var k in known.Take(4)) Console.WriteLine($"  time={k.Time:F1}s pos=({k.X:F0},{k.Z:F0})");
        }
        catch (FileNotFoundException) { Console.WriteLine("无 ETS2NavSemaphore——先让插件定位"); return 1; }
        if (known.Count < 2) { Console.WriteLine("活动灯不足，等灯变化后重试"); return 1; }

        var best = known.OrderByDescending(k => known.Min(o => Math.Abs(o.Time - k.Time))).First();
        Console.WriteLine($"首选灯 time={best.Time:F3}s pos=({best.X:F0},{best.Z:F0})");
        float target = best.Time, tx = best.X, tz = best.Z;

        // v5：精确世界坐标匹配——偏移 (6602,-2923) 由车辆坐标校准（灯在车旁）
        // 每盏灯：wx = lx+6602, wz = lz-2923，±0.5m 匹配 + 窗口内 float∈[0,60]
        const float OX = 6602f, OZ = -2923f;
        var wl = known.Take(10).Select(k => (k.X + OX, k.Z + OZ)).ToList();
        Console.WriteLine($"扫描 {wl.Count} 盏灯的精确世界坐标（±0.5m）");
        var hits = new List<long>();
        IntPtr addr = new(0x100000000);
        var scanBuf = new byte[8192];
        while (addr.ToInt64() < 0x7FFFFFFF0000)
        {
            MEMORY_BASIC_INFORMATION mbi;
            if (VirtualQueryEx(proc, addr, out mbi, (IntPtr)Marshal.SizeOf<MEMORY_BASIC_INFORMATION>()) == false) break;
            IntPtr regionStart = mbi.BaseAddress;
            long regionSize = mbi.RegionSize.ToInt64();
            addr = new IntPtr(regionStart.ToInt64() + regionSize);
            if (mbi.State != MEM_COMMIT) continue;
            uint prot = mbi.Protect;
            if (prot == PAGE_NOACCESS || (prot & PAGE_GUARD) != 0) continue;
            if (prot != 0x04 && prot != 0x02 && prot != 0x08 && prot != 0x40 && prot != 0x20 && prot != 0x84) continue;
            if (regionStart.ToInt64() < 0x100000000) continue;

            for (long off = 0; off + 8192 <= regionSize; off += 8192)
            {
                IntPtr rp;
                if (!ReadProcessMemory(proc, new IntPtr(regionStart.ToInt64() + off), scanBuf, (IntPtr)8192, out rp) || rp.ToInt64() < 8192) break;
                for (int i = 0; i + 4 <= 8192; i += 4)
                {
                    float f = BitConverter.ToSingle(scanBuf, i);
                    foreach (var (wx, wz) in wl)
                    {
                        if (Math.Abs(f - wx) > 0.5f) continue;
                        int w = 16;
                        int s = Math.Max(0, i / 4 - w), e = Math.Min(2047, i / 4 + w);
                        bool zOk = false, tOk = false;
                        for (int j = s; j <= e; j++)
                        {
                            float fv = BitConverter.ToSingle(scanBuf, j * 4);
                            if (Math.Abs(fv - wz) < 0.5f) zOk = true;
                            if (fv > 0.2f && fv < 60f) tOk = true;
                        }
                        if (zOk && tOk)
                        {
                            hits.Add(regionStart.ToInt64() + off + i);
                            break;
                        }
                    }
                }
            }
        }
        Console.WriteLine($"精确坐标候选 {hits.Count} 处");
        foreach (var h in hits.Take(25))
        {
            var dump = new byte[256];
            IntPtr rp2;
            ReadProcessMemory(proc, new IntPtr(h - 128), dump, (IntPtr)256, out rp2);
            var floats = new List<string>();
            for (int k = 0; k < 256; k += 4)
                floats.Add(BitConverter.ToSingle(dump, k).ToString("F2"));
            Console.WriteLine($"  0x{h:x}");
            Console.WriteLine($"    f: {string.Join(" ", floats.Take(32))}");
        }
        CloseHandle(proc);
        return 0;
    }

    static byte[] ReadU32(IntPtr proc, long addr)
    {
        var b = new byte[4];
        IntPtr rp;
        ReadProcessMemory(proc, new IntPtr(addr), b, (IntPtr)4, out rp);
        return b;
    }
}
