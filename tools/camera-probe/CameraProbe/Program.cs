// camera-probe（P3-05 §42）：测速摄像头编码覆盖率验证。
// 扫描 ①归档 def/ 下 camera 相关定义文件 ②Europe sectors 的 prefab token。
// 用法: camera-probe [--install <游戏根目录>]
// GPL-3.0 — ETS2Nav 项目

using ScsResource;
using ScsSector;

var cmdArgs = Environment.GetCommandLineArgs().Skip(1).ToArray();
var installDir = Arg(cmdArgs, "--install") ?? @"E:\SteamLibrary\steamapps\common\Euro Truck Simulator 2";
var install = GameInstall.Detect(installDir);
Console.WriteLine($"游戏 v{install.GameVersion}，{install.Archives.Count} archives / {install.EnabledDlc.Count} DLC");
using var overlay = install.BuildOverlay();

// ── 1) def/ 下 camera 相关文件 ──
Console.WriteLine("\n== def/ 含 camera 的文件 ==");
var camFiles = new List<string>();
try
{
    camFiles = overlay.Enumerate("/def")
        .Where(p => p.Contains("camera", StringComparison.OrdinalIgnoreCase))
        .OrderBy(p => p)
        .ToList();
}
catch (Exception e)
{
    Console.WriteLine($"def 枚举失败: {e.Message}");
}
foreach (var f in camFiles) Console.WriteLine($"  {f}");

// ── 2) 读取 camera 相关 SII 内容（前几个文件）──
Console.WriteLine("\n== camera SII 内容抽样 ==");
foreach (var f in camFiles.Take(6))
{
    try
    {
        using var s = overlay.Open(f);
        using var r = new StreamReader(s);
        var head = string.Join('\n', Enumerable.Range(0, 12).Select(_ => r.ReadLine() ?? ""));
        Console.WriteLine($"--- {f} ---");
        Console.WriteLine(head);
    }
    catch (Exception e) { Console.WriteLine($"  {f}: {e.Message}"); }
}

// ── 3) Europe sectors prefab token 扫描 ──
Console.WriteLine("\n== Europe sectors prefab token 含 camera ==");
var secNames = overlay.Enumerate("/map/europe")
    .Where(p => p.EndsWith(".base") && (p.Contains("/sec+") || p.Contains("/sec-")))
    .Select(p => Path.GetFileNameWithoutExtension(p))
    .OrderBy(n => n)
    .ToArray();
Console.WriteLine($"sectors: {secNames.Length}");
var camTokens = new Dictionary<string, int>();   // token -> 实例数
long prefabTotal = 0;
foreach (var name in secNames)
{
    using var s = overlay.Open($"/map/europe/{name}.base");
    var sec = SectorFile.Read(s, name);
    foreach (var p in sec.Items.OfType<PrefabItem>())
    {
        prefabTotal++;
        if (p.Model.Contains("camera", StringComparison.OrdinalIgnoreCase)
            || p.Model.Contains("cam_", StringComparison.OrdinalIgnoreCase))
            camTokens[p.Model] = camTokens.GetValueOrDefault(p.Model) + 1;
    }
}
Console.WriteLine($"prefab 实例总数: {prefabTotal}；含 camera 关键字 token 数: {camTokens.Count}");
foreach (var (k, v) in camTokens.OrderByDescending(x => x.Value))
    Console.WriteLine($"  {k}: {v} 实例");

// ── 4) ModelItem token 扫描（P3-05：扩展 SectorFile.ReadModel 保留 name token）──
Console.WriteLine("\n== Europe sectors model item token 含 camera ==");
var camModelTokens = new Dictionary<string, int>();
long modelTotal = 0;
foreach (var name in secNames)
{
    using var s = overlay.Open($"/map/europe/{name}.base");
    var sec = SectorFile.Read(s, name);
    foreach (var m in sec.Items.OfType<MapItem>().Where(x => x.Type == ItemType.Model))
    {
        modelTotal++;
        var t = m.Token ?? "";
        if (t.Contains("camera", StringComparison.OrdinalIgnoreCase)
            || t.Contains("cam_", StringComparison.OrdinalIgnoreCase)
            || t.Contains("speedcam", StringComparison.OrdinalIgnoreCase)
            || t.Contains("radar", StringComparison.OrdinalIgnoreCase))
            camModelTokens[t] = camModelTokens.GetValueOrDefault(t) + 1;
    }
}
Console.WriteLine($"model 实例总数: {modelTotal}；命中 token 数: {camModelTokens.Count}");
foreach (var (k, v) in camModelTokens.OrderByDescending(x => x.Value))
    Console.WriteLine($"  {k}: {v} 实例");

// ── 5) def/world 文本内容扫描（road look variant / speed camera 定义）──
Console.WriteLine("\n== def/world 文本含 speed_camera/speedcam/radar ==");
var hits = new Dictionary<string, List<string>>();
foreach (var f in overlay.Enumerate("/def/world"))
{
    if (!f.EndsWith(".sii") && !f.EndsWith(".sui") && !f.EndsWith(".txt")) continue;
    try
    {
        using var s = overlay.Open(f);
        using var r = new StreamReader(s);
        var txt = r.ReadToEnd();
        foreach (var kw in new[] { "speed_camera", "speedcam", "speed_cam", "radar" })
        {
            if (txt.Contains(kw, StringComparison.OrdinalIgnoreCase))
            {
                if (!hits.ContainsKey(kw)) hits[kw] = new List<string>();
                hits[kw].Add(f);
            }
        }
    }
    catch { }
}
foreach (var (kw, files) in hits)
{
    Console.WriteLine($"  '{kw}': {files.Count} 个文件");
    foreach (var f in files.Take(8)) Console.WriteLine($"    {f}");
}
if (hits.Count == 0) Console.WriteLine("  （无命中）");

// ── 6) Europe sectors SignItem 扫描（sign model/variant 含 speed_camera）──
Console.WriteLine("\n== Europe sectors sign item（speed_camera 系）==");
var camSigns = new Dictionary<string, int>();
long signTotal = 0;
foreach (var name in secNames)
{
    using var s = overlay.Open($"/map/europe/{name}.base");
    var sec = SectorFile.Read(s, name);
    foreach (var sg in sec.Items.OfType<SignItem>())
    {
        signTotal++;
        var key = $"{sg.SignModel}#{sg.Variant}";
        if (sg.SignModel.Contains("speed_camera", StringComparison.OrdinalIgnoreCase)
            || sg.SignModel.Contains("speedcam", StringComparison.OrdinalIgnoreCase)
            || sg.Variant.Contains("speed_camera", StringComparison.OrdinalIgnoreCase)
            || sg.Variant.Contains("speedcam", StringComparison.OrdinalIgnoreCase)
            || sg.Variant.Contains("camera", StringComparison.OrdinalIgnoreCase))
            camSigns[key] = camSigns.GetValueOrDefault(key) + 1;
    }
}
Console.WriteLine($"sign 实例总数: {signTotal}；命中: {camSigns.Count} 种 token");
foreach (var (k, v) in camSigns.OrderByDescending(x => x.Value).Take(20))
    Console.WriteLine($"  {k}: {v} 实例");
if (camSigns.Count == 0) Console.WriteLine("  （无命中——sign 模型名不含 speed_camera）");

string? Arg(string[] a, string key)
{
    for (int i = 0; i < a.Length - 1; i++)
        if (a[i] == key) return a[i + 1];
    return null;
}
