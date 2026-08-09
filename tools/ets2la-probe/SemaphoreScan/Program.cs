// semaphore-scan v6：强约束静态过滤（四元数归一化等）→ 动态跟踪验证
using System.Diagnostics;
using System.Runtime.InteropServices;

var proc = Process.GetProcessesByName("eurotrucks2").FirstOrDefault();
if (proc is null) { Console.WriteLine("未找到 eurotrucks2 进程"); Console.ReadKey(); return; }
Console.WriteLine($"PID={proc.Id}");

int[] validStates = [1, 2, 4, 8, 32];   // 去掉 0（OFF 罕见）
var rawCandidates = new List<long>();
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
        if (time is < 0 or > 300) continue;
        // 四元数归一化：|q|^2 = x^2+y^2+z^2+w^2 ≈ 1（最强约束）
        float qx = BitConverter.ToSingle(buf, i + 16), qy = BitConverter.ToSingle(buf, i + 20);
        float qz = BitConverter.ToSingle(buf, i + 24), qw = BitConverter.ToSingle(buf, i + 28);
        float q2 = qx * qx + qy * qy + qz * qz + qw * qw;
        if (q2 < 0.8f || q2 > 1.2f) continue;
        // position.Y 合理
        float py = BitConverter.ToSingle(buf, i + 4);
        if (float.IsNaN(py) || float.IsInfinity(py) || Math.Abs(py) > 100000) continue;
        // 排除零填充
        bool zeroSlot = BitConverter.ToInt32(buf, i) == 0 && BitConverter.ToInt32(buf, i + 8) == 0 && time == 0 && state == 0;
        if (zeroSlot) continue;
        rawCandidates.Add((long)mbi.BaseAddress + i);
    }
}
Console.WriteLine($"强约束静态候选：{rawCandidates.Count} 处（扫描 {scanned / 1024 / 1024} MB）");

// 动态跟踪（仅当候选可控时）
const int frames = 100;
if (rawCandidates.Count > 0 && rawCandidates.Count <= 20000)
{
    var tracks = rawCandidates.Select(a => new Track(a)).ToList();
    var sw = Stopwatch.StartNew();
    for (int f = 0; f < frames; f++)
    {
        for (int k = 0; k < tracks.Count; k++)
        {
            var one = new byte[48];
            if (Native.ReadProcessMemory(proc.Handle, (IntPtr)tracks[k].Addr, one, 48, out _))
            {
                tracks[k].States[f] = BitConverter.ToInt32(one, 44);
                tracks[k].Times[f] = BitConverter.ToSingle(one, 40);
                tracks[k].Valid++;
            }
        }
        if (f < frames - 1) Thread.Sleep(100);
    }
    Console.WriteLine($"跟踪完成（{sw.Elapsed.TotalSeconds:F1}s）");

    var confirmed = new List<(long Addr, int Transitions, float MaxDrop)>();
    foreach (var c in tracks)
    {
        if (c.Valid < 30) continue;
        int transitions = 0;
        for (int f = 1; f < frames; f++)
            if (c.States[f] != 0 && c.States[f - 1] != 0 && c.States[f] != c.States[f - 1])
                transitions++;
        float maxDrop = 0;
        for (int f = 1; f < frames; f++)
        {
            float a = c.Times[f - 1], b = c.Times[f];
            if (a > 0.5f && b >= 0 && a - b > maxDrop) maxDrop = a - b;
        }
        if (transitions >= 1 || maxDrop > 2.0f)
            confirmed.Add((c.Addr, transitions, maxDrop));
    }
    Console.WriteLine($"状态机验证通过：{confirmed.Count} 处");
    foreach (var (a, tr, md) in confirmed.Take(30))
        Console.WriteLine($"  0x{a:x12}  转换×{tr}  最大递减 {md:F1}s");

    var outPath = Path.Combine(AppContext.BaseDirectory, "semaphore-scan-results.txt");
    File.WriteAllLines(outPath, confirmed.Select(c => $"0x{c.Addr:x12} t={c.Transitions} drop={c.MaxDrop:F1}"));
    Console.WriteLine($"结果已保存：{outPath}（{confirmed.Count} 处）");
}
else
{
    Console.WriteLine(rawCandidates.Count == 0 ? "无静态候选（布局假设可能错误）" : $"候选 {rawCandidates.Count} 过多，跳过跟踪（需进一步收紧）");
    var outPath = Path.Combine(AppContext.BaseDirectory, "semaphore-scan-results.txt");
    File.WriteAllLines(outPath, rawCandidates.Take(200).Select(a => $"0x{a:x12}"));
    Console.WriteLine($"前 200 个候选已保存：{outPath}");
}
Console.WriteLine("按任意键退出...");
Console.ReadKey();

sealed class Track
{
    public long Addr;
    public int[] States = new int[100];
    public float[] Times = new float[100];
    public int Valid;
    public Track(long a) { Addr = a; }
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
