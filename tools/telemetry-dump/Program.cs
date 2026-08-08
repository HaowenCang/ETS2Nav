// telemetry-dump：读取 scs-nav-bridge 共享内存并打印遥测（P0 工具，v0.2 §74）。
// 布局与 telemetry-plugin/scs-nav-bridge/scs-nav-bridge.cpp 中 telemetry_state_t 对应。
// 用法：运行 ETS2 后执行本工具；Ctrl+C 退出。

using System.Buffers.Binary;
using System.IO.MemoryMappedFiles;
using System.Text;

const string MapName = "Local\\ETS2NavTelemetry";
const uint LayoutVersion = 1;

using var mmf = MemoryMappedFile.OpenExisting(MapName);
using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);

var lastSequence = -1u;
var lastPrint = DateTime.MinValue;

Console.WriteLine("telemetry-dump：监听 ETS2NavTelemetry（Ctrl+C 退出）");
Console.WriteLine("布局版本校验与字段：sequence/layout/时间戳/位置/速度/限速/燃油/任务");

while (true)
{
    uint sequence = view.ReadUInt32(0);
    uint version = view.ReadUInt32(4);
    if (version != LayoutVersion)
    {
        Console.WriteLine($"[错误] 布局版本不匹配：共享内存 {version}，本工具 {LayoutVersion}");
        break;
    }
    if (sequence != lastSequence && (DateTime.Now - lastPrint).TotalMilliseconds >= 500)
    {
        lastSequence = sequence;
        lastPrint = DateTime.Now;
        PrintState(view);
    }
    Thread.Sleep(20);
}

static void PrintState(MemoryMappedViewAccessor v)
{
    byte running = v.ReadByte(8);
    byte paused = v.ReadByte(9);
    ulong simTime = v.ReadUInt64(0x0C);
    ulong pausedSim = v.ReadUInt64(0x14);
    uint gameTime = v.ReadUInt32(0x2C);
    float localScale = v.ReadSingle(0x30);
    int restStop = v.ReadInt32(0x34);

    double px = v.ReadDouble(0x38);
    double py = v.ReadDouble(0x40);
    double pz = v.ReadDouble(0x48);
    float heading = v.ReadSingle(0x50);
    float pitch = v.ReadSingle(0x54);
    float roll = v.ReadSingle(0x58);

    float speed = v.ReadSingle(0x60);
    float speedLimit = v.ReadSingle(0x64);
    float fuel = v.ReadSingle(0x68);
    float fuelRange = v.ReadSingle(0x6C);
    byte fuelWarning = v.ReadByte(0x70);
    byte jobActive = v.ReadByte(0x71);

    string srcCity = ReadString(v, 0x72, 64);
    string srcCityId = ReadString(v, 0x72 + 64, 64);
    string srcCompany = ReadString(v, 0x72 + 128, 64);
    string srcCompanyId = ReadString(v, 0x72 + 192, 64);
    string dstCity = ReadString(v, 0x72 + 256, 64);
    string dstCityId = ReadString(v, 0x72 + 320, 64);
    string dstCompany = ReadString(v, 0x72 + 384, 64);
    string dstCompanyId = ReadString(v, 0x72 + 448, 64);
    uint income = v.ReadUInt32(0x272);
    uint deliveryTime = v.ReadUInt32(0x276);

    Console.WriteLine($"--- seq={v.ReadUInt32(0)} run={running} paused={paused} ---");
    Console.WriteLine($"sim={simTime / 1_000_000.0:F3}s pausedSim={pausedSim / 1_000_000.0:F3}s");
    Console.WriteLine($"game.time={gameTime}min scale={localScale:F3} rest.stop={restStop}min");
    Console.WriteLine($"pos=({px:F1},{py:F1},{pz:F1}) heading={heading:F3} pitch={pitch:F3} roll={roll:F3}");
    Console.WriteLine($"speed={speed * 3.6:F1}km/h limit={(speedLimit > 0 ? speedLimit * 3.6 : 0):F0}km/h fuel={fuel:F0}L range={fuelRange:F0}km warn={fuelWarning}");
    Console.WriteLine($"job={jobActive} src={srcCity}/{srcCompany}({srcCompanyId}) dst={dstCity}/{dstCompany}({dstCompanyId}) income={income} delivery={deliveryTime}min");
}

static string ReadString(MemoryMappedViewAccessor v, long offset, int maxLen)
{
    var bytes = new byte[maxLen];
    v.ReadArray(offset, bytes, 0, maxLen);
    int len = Array.IndexOf(bytes, (byte)0);
    if (len < 0) len = maxLen;
    return Encoding.UTF8.GetString(bytes, 0, len);
}
