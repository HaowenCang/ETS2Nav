// memscan-probe：扫描 eurotrucks2 进程内存中的 ETS2LA/Local 共享内存名（探针，只读）

using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;

var proc = Process.GetProcessesByName("eurotrucks2").FirstOrDefault();
if (proc is null) { Console.WriteLine("未找到 eurotrucks2"); return; }
Console.WriteLine($"PID={proc.Id}");

var ets2la = Encoding.ASCII.GetBytes("ETS2LA");
var local = Encoding.ASCII.GetBytes("Local\\");
var seen = new HashSet<string>();
var addr = IntPtr.Zero;
long scanned = 0;
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
    for (int i = 0; i + 8 <= buf.Length; i++)
    {
        bool hit = buf.AsSpan(i, 6).SequenceEqual(ets2la) || buf.AsSpan(i, 6).SequenceEqual(local);
        if (!hit) continue;
        int end = i;
        while (end < buf.Length && buf[end] >= 0x20 && buf[end] < 0x7f) end++;
        string s = Encoding.ASCII.GetString(buf, i, Math.Min(end - i, 120));
        if (s.Length >= 8 && seen.Add(s))
            Console.WriteLine($"0x{((long)mbi.BaseAddress + i):x12}  {s}");
        i = end - 1;
    }
}
Console.WriteLine($"done（扫描 {scanned / 1024 / 1024} MB，唯一串 {seen.Count} 条）");

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
