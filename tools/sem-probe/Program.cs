// sem-head：读取共享内存头部原始值（诊断）
using System.IO.MemoryMappedFiles;
try
{
    using var mmf = MemoryMappedFile.OpenExisting(@"Local\ETS2NavSemaphore");
    using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);
    uint magic = view.ReadUInt32(0);
    uint version = view.ReadUInt32(4);
    uint seq = view.ReadUInt32(8);
    uint count = view.ReadUInt32(12);
    Console.WriteLine($"magic=0x{magic:x8} (期望 0x324d4553)  version={version}  seq={seq}  count={count}");
    // 等待 3 秒看 seq 是否变化
    for (int i = 0; i < 6; i++)
    {
        Thread.Sleep(500);
        uint s2 = view.ReadUInt32(8);
        Console.WriteLine($"  t+{(i + 1) * 0.5:F1}s seq={s2}");
    }
}
catch (FileNotFoundException) { Console.WriteLine("共享内存不存在——插件未加载"); }
