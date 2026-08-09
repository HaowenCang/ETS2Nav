// sem-probe：读取 ETS2NavSemaphore（自研信号灯共享内存）并验证
// 布局：magic "SEM2"(4) + version(4) + sequence(4) + count(4) + count×48B
// 48B/灯：pos(12) + cx/cy(4) + quat(16) + type(4) + time(4) + state(4) + id(4)
using System.IO.MemoryMappedFiles;

const string MapName = @"Local\ETS2NavSemaphore";
const int LightSize = 48;

using var mmf = MemoryMappedFile.OpenExisting(MapName);
using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);

string StateName(int s) => s switch
{
    0 => "OFF", 1 => "ORANGE_TO_RED", 2 => "RED", 4 => "ORANGE_TO_GREEN", 8 => "GREEN", 32 => "SLEEP",
    _ => $"?({s})"
};

Console.WriteLine("sem-probe：ETS2NavSemaphore（Ctrl+C 退出）");
uint lastSeq = 0;
while (true)
{
    uint magic = view.ReadUInt32(0);
    uint version = view.ReadUInt32(4);
    uint seq = view.ReadUInt32(8);
    uint count = view.ReadUInt32(12);
    if (magic != 0x324D4553) { Console.WriteLine("magic 错误"); break; }
    if (seq != lastSeq)
    {
        lastSeq = seq;
        Console.WriteLine($"--- seq={seq} count={count} ---");
        var buf = new byte[count * LightSize];
        view.ReadArray(16, buf, 0, buf.Length);
        for (int i = 0; i < count; i++)
        {
            int o = i * LightSize;
            var pos = (BitConverter.ToSingle(buf, o), BitConverter.ToSingle(buf, o + 4), BitConverter.ToSingle(buf, o + 8));
            short cx = BitConverter.ToInt16(buf, o + 12), cy = BitConverter.ToInt16(buf, o + 14);
            int type = BitConverter.ToInt32(buf, o + 32);
            float time = BitConverter.ToSingle(buf, o + 36);
            int state = BitConverter.ToInt32(buf, o + 40);
            int id = BitConverter.ToInt32(buf, o + 44);
            Console.WriteLine($"{id,3} | {type} | {StateName(state),-15} | {time,6:F1}s | ({pos.Item1:F1},{pos.Item2:F1},{pos.Item3:F1}) | {cx},{cy}");
        }
    }
    Thread.Sleep(200);
}
