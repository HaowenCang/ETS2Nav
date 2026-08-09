// vision-anchor：低成本区域视觉锚定器（P0-B 锚定源）
// 原理（TL-01/02）：信号灯由 simulation_time 驱动（interval 秒=真实秒）、相位锚定于加载时刻
//   → 观测到一次相位转换即可外推整个周期。
// 低成本：只采样信号灯位置小区域（60×60 px）@10Hz + HSV 阈值 → 状态转换即锚点。
// 校准：鼠标移到信号灯灯头上按 F1；S 开始/停止采样；Q 退出。
// 用法：游戏运行（信号灯可见）+ scs-nav-bridge 插件加载时执行。

using System.Drawing;
using System.Drawing.Imaging;
using System.IO.MemoryMappedFiles;
using System.Runtime.InteropServices;

const int SampleSize = 60;
const int SampleIntervalMs = 100;

[DllImport("user32.dll")]
static extern IntPtr GetDC(IntPtr hwnd);
[DllImport("user32.dll")]
static extern int ReleaseDC(IntPtr hwnd, IntPtr hdc);
[DllImport("gdi32.dll")]
static extern int BitBlt(IntPtr hdcDest, int x, int y, int w, int h, IntPtr hdcSrc, int x1, int y1, int rop);

Console.WriteLine("vision-anchor：低成本区域视觉锚定器");
Console.WriteLine("校准：鼠标移到信号灯灯头上按 F1；S 开始/停止采样；Q 退出");

MemoryMappedFile? mmf = null;
MemoryMappedViewAccessor? view = null;
try
{
    mmf = MemoryMappedFile.OpenExisting(@"Local\ETS2NavTelemetry");
    view = mmf.CreateViewAccessor(0, 0, MemoryMappedFileAccess.Read);
}
catch (FileNotFoundException)
{
    Console.WriteLine("[提示] scs-nav-bridge 插件未加载——simulation_time 基准不可用");
}

(int X, int Y)? anchor = null;
bool sampling = false;
bool stop = false;

var keyThread = new Thread(() =>
{
    while (!stop)
    {
        if (Console.KeyAvailable)
        {
            var key = Console.ReadKey(true);
            switch (key.Key)
            {
                case ConsoleKey.F1:
                    var p = System.Windows.Forms.Cursor.Position;
                    anchor = (p.X, p.Y);
                    Console.WriteLine($"[校准] 采样中心 = ({p.X}, {p.Y})");
                    break;
                case ConsoleKey.S:
                    sampling = !sampling;
                    Console.WriteLine(sampling ? "[采样开始]" : "[采样结束]");
                    break;
                case ConsoleKey.Q:
                    stop = true;
                    break;
            }
        }
        Thread.Sleep(10);
    }
});
keyThread.IsBackground = true;
keyThread.Start();

string Classify(byte r, byte g, byte b)
{
    float max = Math.Max(r, Math.Max(g, b));
    float min = Math.Min(r, Math.Min(g, b));
    float delta = max - min;
    if (max < 80) return "OFF";
    float h = 0;
    if (delta > 0)
    {
        if (max == r) h = 60 * (((g - b) / delta) % 6);
        else if (max == g) h = 60 * ((b - r) / delta + 2);
        else h = 60 * ((r - g) / delta + 4);
    }
    if (h < 0) h += 360;
    float s = max == 0 ? 0 : delta / max;
    if (s < 0.3f) return "WHITE";
    if (h < 20 || h >= 340) return "RED";
    if (h < 60) return "YELLOW";
    if (h < 170) return "GREEN";
    return "RED";
}

string? lastState = null;
double? anchorSim = null;

while (!stop)
{
    if (sampling && anchor is { } a)
    {
        int x = a.X - SampleSize / 2, y = a.Y - SampleSize / 2;
        using var bmp = new Bitmap(SampleSize, SampleSize);
        using (var g = Graphics.FromImage(bmp))
            g.CopyFromScreen(x, y, 0, 0, new Size(SampleSize, SampleSize));

        int rc = 0, gc = 0, yc = 0, oc = 0;
        var data = bmp.LockBits(new Rectangle(0, 0, SampleSize, SampleSize), ImageLockMode.ReadOnly, PixelFormat.Format24bppRgb);
        var buf = new byte[data.Stride * SampleSize];
        Marshal.Copy(data.Scan0, buf, 0, buf.Length);
        bmp.UnlockBits(data);
        for (int i = 0; i < buf.Length; i += 3)
        {
            string c = Classify(buf[i + 2], buf[i + 1], buf[i]);
            switch (c)
            {
                case "RED": rc++; break;
                case "GREEN": gc++; break;
                case "YELLOW": yc++; break;
                case "OFF": oc++; break;
            }
        }
        string state = rc > gc && rc > yc ? "RED" : gc > yc ? "GREEN" : yc > 0 ? "YELLOW" : "OFF";
        if (rc > 0 || gc > 0 || yc > 0)
        {
            if (state != lastState)
            {
                double sim = view is not null ? view.ReadUInt64(0x0C) / 1e6 : 0;
                Console.WriteLine($"[转换] {lastState ?? "?"} -> {state}  sim={sim:F3}s  (红{rc} 绿{gc} 黄{yc} 暗{oc})");
                if (lastState is "RED" or "GREEN" or "YELLOW" && state is "RED" or "GREEN" or "YELLOW")
                {
                    anchorSim = sim;
                    Console.WriteLine($"[锚定] 相位已锚定 @ sim={sim:F3}s，可外推倒计时");
                }
                lastState = state;
            }
            else if (anchorSim is { } asim)
            {
                double sim = view is not null ? view.ReadUInt64(0x0C) / 1e6 : 0;
                double phase = (sim - asim) % 60.0;
                if (phase < 0) phase += 60;
                string hint = phase < 33 ? $"红灯剩 {33 - phase:F1}s" : $"绿灯中（{(60 - phase):F1}s 后转红）";
                Console.Write($"\r  相位 {phase:F1}s/60s  {hint}    ");
            }
        }
        else if (lastState != null)
        {
            lastState = null;
        }
    }
    Thread.Sleep(SampleIntervalMs);
}
Console.WriteLine("\n退出。");
