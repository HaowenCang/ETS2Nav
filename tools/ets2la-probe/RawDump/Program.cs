// raw-dump v2：尝试多种布局解释（定位真实字段偏移）
using System.IO.MemoryMappedFiles;

using var mmf = MemoryMappedFile.OpenExisting(@"Local\ETS2LASemaphore");
using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);
var buf = new byte[2080];
view.ReadArray(0, buf, 0, buf.Length);

// 布局 A：type@32 time@36 state@40 id@44（48B）
Console.WriteLine("布局A: pos(12)+cx/cy(4)+quat(16)+type(4)+time(4)+state(4)+id(4)");
for (int i = 0; i < 6; i++)
{
    int o = i * 48;
    var pos = (BitConverter.ToSingle(buf, o), BitConverter.ToSingle(buf, o + 4), BitConverter.ToSingle(buf, o + 8));
    short cx = BitConverter.ToInt16(buf, o + 12), cy = BitConverter.ToInt16(buf, o + 14);
    var q = (BitConverter.ToSingle(buf, o + 16), BitConverter.ToSingle(buf, o + 20), BitConverter.ToSingle(buf, o + 24), BitConverter.ToSingle(buf, o + 28));
    int type = BitConverter.ToInt32(buf, o + 32);
    float time = BitConverter.ToSingle(buf, o + 36);
    int state = BitConverter.ToInt32(buf, o + 40);
    int id = BitConverter.ToInt32(buf, o + 44);
    Console.WriteLine($"槽{i}: pos=({pos.Item1:F1},{pos.Item2:F1},{pos.Item3:F1}) cx={cx} cy={cy} q=({q.Item1:F2},{q.Item2:F2},{q.Item3:F2},{q.Item4:F2}) type={type} time={time:F2} state={state} id={id}");
}
