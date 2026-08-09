// sem-probe v2 原版（用户工具）——从 git 恢复
using System.IO.MemoryMappedFiles;
using System.Text;
var mmf = MemoryMappedFile.OpenExisting(@"Local\ETS2NavSemaphore");
using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);
Console.WriteLine($"sem-probe: ETS2NavSemaphore (Ctrl+C 退出)");
Console.WriteLine($"magic={view.ReadUInt32(0):x8} version={view.ReadUInt32(4)} seq={view.ReadUInt32(8)} count={view.ReadUInt32(12)}");
uint last = 0;
while (true)
{
    uint seq = view.ReadUInt32(8);
    uint count = view.ReadUInt32(12);
    if (seq != last && count <= 64)
    {
        last = seq;
        var buf = new byte[(int)count * 48];
        view.ReadArray(16, buf, 0, buf.Length);
        Console.WriteLine($"--- seq={seq} count={count} ---");
        for (int i = 0; i < count; i++)
        {
            int o = i * 48;
            int type = BitConverter.ToInt32(buf, o + 32);
            int state = BitConverter.ToInt32(buf, o + 40);
            float time = BitConverter.ToSingle(buf, o + 36);
            if (type == 0 && state == 0) continue;
            float px = BitConverter.ToSingle(buf, o), pz = BitConverter.ToSingle(buf, o + 8);
            Console.WriteLine($"  [{i}] type={type} state={state,-5} time={time,5:F1}s pos=({px,8:F1},{pz,8:F1})");
        }
    }
    Thread.Sleep(50);
}
