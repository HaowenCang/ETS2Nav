using ScsResource;

try
{
    var install = GameInstall.Detect(@"E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2");
    using var overlay = install.BuildOverlay();
    string ReadSii(string p)
    {
        using var s = overlay.Open(p);
        using var r = new StreamReader(s);
        return r.ReadToEnd();
    }
    var t = ReadSii("/def/world/road_look.template.sii");
    var lines = t.Split('\n');
    Console.WriteLine($"总行数 {lines.Length}");
    for (int i = 0; i < lines.Length; i++)
    {
        if (lines[i].Contains("road.template0"))
        {
            Console.WriteLine($"--- road.template0 @行 {i + 1} ---");
            for (int j = i; j < Math.Min(lines.Length, i + 45); j++)
                Console.WriteLine(lines[j].TrimEnd('\r'));
            break;
        }
    }
}
catch (Exception ex) { Console.WriteLine($"FAIL: {ex}"); }
