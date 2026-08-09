// hook-scan：对比游戏内存代码段与磁盘文件，找出被 hook（修改）的字节
using System.Diagnostics;
using System.Runtime.InteropServices;

class Program
{
    [DllImport("kernel32.dll")] static extern IntPtr OpenProcess(uint a, bool i, uint p);
    [DllImport("kernel32.dll")] static extern bool ReadProcessMemory(IntPtr h, IntPtr a, byte[] b, IntPtr s, out IntPtr r);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

    static int Main()
    {
        var p = Process.GetProcessesByName("eurotrucks2").FirstOrDefault();
        if (p == null) { Console.WriteLine("游戏未运行"); return 1; }
        var proc = OpenProcess(0x0010 | 0x0400, false, (uint)p.Id);
        string exePath = p.MainModule.FileName;
        var disk = File.ReadAllBytes(exePath);
        long baseAddr = p.MainModule.BaseAddress.ToInt64();
        long textSize = p.MainModule.ModuleMemorySize;
        // .text 段通常在文件偏移 0x400-0x500（PE 头后），内存中 .text 从 baseAddr+0x1000 起
        long memTextOff = 0x1000;
        long fileTextOff = 0x400;
        long len = Math.Min(textSize - memTextOff, disk.Length - fileTextOff);
        Console.WriteLine($"exe={exePath} 基址=0x{baseAddr:x} 内存段 {len} 字节");
        var mem = new byte[len];
        IntPtr rp;
        bool ok = ReadProcessMemory(proc, new IntPtr(baseAddr + memTextOff), mem, (IntPtr)len, out rp);
        if (!ok || rp.ToInt64() < len) { Console.WriteLine($"读取失败 ok={ok} rp={rp.ToInt64()}"); return 1; }
        // 对比（跳过明显静态差异段：统计差异分布）
        var diffs = new List<(long Off, byte D, byte M)>();
        for (long i = 0; i < len; i++)
        {
            byte d = disk[fileTextOff + i], m = mem[i];
            if (d != m) diffs.Add((i, d, m));
        }
        Console.WriteLine($"差异字节 {diffs.Count} 个");
        // 聚类：差异连续区域（hook 通常 5-40 字节）
        long last = -100;
        int groupStart = 0;
        foreach (var d in diffs)
        {
            if (d.Off - last > 64) { if (groupStart >= 0 && d.Off != 0) { } groupStart = (int)d.Off; }
            last = d.Off;
        }
        // 分组输出
        var groups = new List<(long Start, int Count)>();
        long gs = -1; int gc = 0; long prev = -100;
        foreach (var d in diffs)
        {
            if (d.Off - prev > 64) { if (gc > 0) groups.Add((gs, gc)); gs = d.Off; gc = 0; }
            gc++; prev = d.Off;
        }
        if (gc > 0) groups.Add((gs, gc));
        Console.WriteLine($"差异簇 {groups.Count} 个（>64B 间隔分组）");
        int shown = 0;
        foreach (var g in groups)
        {
            if (shown++ >= 30) break;
            var hex = string.Join(" ", diffs.Where(d => d.Off >= g.Start && d.Off < g.Start + g.Count + 100).Take(48).Select(d => $"+0x{d.Off:x} {d.D:x2}→{d.M:x2}"));
            Console.WriteLine($"  0x{baseAddr + 0x1000 + g.Start:x} 簇{g.Count}字节: {hex}");
        }
        CloseHandle(proc);
        return 0;
    }
}
