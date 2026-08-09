using ScsSector;
foreach (var sec in new[] { "sec+0002-0002", "sec+0002-0003", "sec+0002-0001", "sec+0002-0004", "sec+0003-0001", "sec+0003-0002", "sec+0003-0003", "sec+0003-0004" })
{
    var path = $@"E:\Projects\Pi\ETS2Nav\vendor\extracted\base_map\map\europe\{sec}.aux";
    if (!File.Exists(path)) { Console.WriteLine($"{sec}: 无 aux"); continue; }
    try
    {
        var s = SectorFile.Read(path);
        Console.WriteLine($"{sec}: OK {s.Items.Count} items / {s.Nodes.Count} nodes");
    }
    catch (Exception ex) { Console.WriteLine($"{sec}: FAIL {ex.Message}"); }
}
