// TruckLib oracle：解析 .base/.aux 对照 item 数
using TruckLib.ScsMap;
var path = @"E:\Projects\Pi\ETS2Nav\vendor\extracted\base_map\map\europe\sec+0002-0002.base";
try
{
    var map = Map.Open(path);
    Console.WriteLine($"TruckLib 解析成功: items={map.MapItems.Count} nodes={map.Nodes.Count}");
}
catch (Exception ex) { Console.WriteLine($"FAIL: {ex.GetType().Name} {ex.Message}"); }
