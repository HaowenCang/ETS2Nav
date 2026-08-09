// ets2la-probe：读取 ETS2LA 信号灯共享内存（Local\ETS2LASemaphore），验证数据可用性。
// 布局（ETS2LA.Semaphores.cs 公开定义，48 字节/灯 × 40）：
//   position: 3×float(12) + cx:short(2) + cy:short(2) + rotation: 4×float(16)
//   + type:int(4) + time_remaining:float(4) + state:int(4) + id:int(4)
// 状态枚举：OFF=0, ORANGETORED=1, RED=2, ORANGETOGREEN=4, GREEN=8, SLEEP=32
// 用法：游戏运行（且 ETS2LA 插件加载）时执行，Ctrl+C 退出。

using System.IO.MemoryMappedFiles;
using System.Numerics;

const string MapName = "Local\\ETS2LASemaphore";
const int LightCount = 40;
const int LightSize = 48;

using var mmf = MemoryMappedFile.OpenExisting(MapName);
using var view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);
var buf = new byte[LightCount * LightSize];

Console.WriteLine("ets2la-probe：读取 ETS2LASemaphore（Ctrl+C 退出）");
Console.WriteLine("pos_x,pos_y,pos_z,type,state,time_remaining,id");

while (true)
{
    view.ReadArray(0, buf, 0, buf.Length);
    bool any = false;
    for (int i = 0; i < LightCount; i++)
    {
        int o = i * LightSize;
        var pos = new Vector3(BitConverter.ToSingle(buf, o), BitConverter.ToSingle(buf, o + 4), BitConverter.ToSingle(buf, o + 8));
        int type = BitConverter.ToInt32(buf, o + 36);
        float remaining = BitConverter.ToSingle(buf, o + 40);
        int state = BitConverter.ToInt32(buf, o + 44);
        int id = BitConverter.ToInt32(buf, o + 48 - 4);
        if (type == 0 && state == 0 && remaining == 0 && pos == Vector3.Zero) continue;   // 空槽
        any = true;
        Console.WriteLine($"{pos.X:F1},{pos.Y:F1},{pos.Z:F1},{type},{state},{remaining:F1},{id}");
    }
    if (!any) Console.WriteLine("（无活动信号灯数据——需在信号灯路口附近）");
    Console.WriteLine("---");
    Thread.Sleep(1000);
}
