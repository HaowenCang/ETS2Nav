// dump-lamp：dump 灯对象元素（688B），标注 state 枚举/int/float
using System.Diagnostics;
using System.Runtime.InteropServices;

if (args.Length < 1) { Console.WriteLine("用法: dump-lamp <hex地址> [长度]"); return; }
long baseAddr = Convert.ToInt64(args[0], 16);
int len = args.Length > 1 ? int.Parse(args[1]) : 688;

var proc = Process.GetProcessesByName("eurotrucks2").FirstOrDefault();
if (proc is null) { Console.WriteLine("未找到 eurotrucks2"); return; }

var buf = new byte[len];
if (!Native.ReadProcessMemory(proc.Handle, (IntPtr)baseAddr, buf, len, out _))
{ Console.WriteLine("读取失败"); return; }

Console.WriteLine($"=== dump 0x{baseAddr:x12} ({len}B) ===");
for (int i = 0; i + 4 <= len; i += 4)
{
    int iv = BitConverter.ToInt32(buf, i);
    float f = BitConverter.ToSingle(buf, i);
    string tag = "";
    if (iv is 0 or 1 or 2 or 4 or 8 or 32) tag = $"  <== int={iv}";
    else if (!float.IsNaN(f) && !float.IsInfinity(f) && Math.Abs(f) < 100 && Math.Abs(f) > 0.001) tag = $"  float={f:F2}";
    else if (iv != 0) tag = $"  int={iv}";
    if (tag != "")
        Console.WriteLine($"  +{i:x4}  0x{baseAddr + i:x12}: {iv,12} {f,10:F3}{tag}");
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
