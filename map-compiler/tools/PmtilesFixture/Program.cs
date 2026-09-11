using System.Security.Cryptography;
using System.Text.Json;
using ScsVectorTiles;

namespace PmtilesFixture;

/// <summary>
/// PMTiles 互操作验证夹具生成器（P4R Batch 1.5）。
///
/// 用正式 <see cref="PmtilesWriter"/> + 正式 <see cref="MvtEncoder"/> 生成一个小型
/// 但结构完整的归档，并输出期望值 sidecar（每个瓦片的 z/x/y、tile_id、MVT 明文 SHA-256），
/// 供独立读取器（官方 pmtiles npm 包，见 tools/ets2nav-web/scripts/verify-pmtiles.mjs）
/// 与浏览器 smoke 使用。
///
/// 用法：pmtiles-fixture &lt;out.pmtiles&gt; [--large]
///   --large：生成足以让 root directory 超过 16384 字节限制的规模，用于验证 leaf 分支。
///
/// 注意：本工具的坐标范围刻意覆盖 Web UI 初始视野（app.js 中心 -58400/33000 米），
/// 以便浏览器 smoke 能真实请求到瓦片。
/// </summary>
public static class Program
{
    public static int Main(string[] args)
    {
        if (args.Length < 1)
        {
            Console.Error.WriteLine("用法: pmtiles-fixture <out.pmtiles> [--large] [--minimal]");
            return 2;
        }
        var outPath = args[0];
        bool large = args.Contains("--large");
        bool minimal = args.Contains("--minimal");

        // 覆盖层：与 UI 初始视野一致的经纬度窗口（经度 -1.5..0.5、纬度 -0.5..1.5）
        // --large   加深到 z14，使条目数远超 root directory 的 16384 字节限制，触发 leaf 分支。
        // --minimal 只出 1 块瓦片，使归档小于 PMTiles 客户端的首个 16384 字节探测窗口，
        //           用于验证「探测区间不可满足 -> 416 -> 客户端按 Content-Range 回退」路径。
        int minZoom = minimal ? 6 : 6;
        int maxZoom = large ? 14 : minimal ? 6 : 12;
        double minLng = -1.5, maxLng = 0.5, minLat = -0.5, maxLat = 1.5;
        if (minimal)
        {
            // 收窄到单块瓦片（z6 x33 y21 覆盖经度 5.6..11.25、纬度 40.98..43.07 的调试坐标区）
            var (cx, cy) = MvtEncoder.TileXY(-58400 / 111320.0, 33000 / 111320.0, 6);
            (minLng, minLat) = TileBounds(cx, cy, 6).Min;
            (maxLng, maxLat) = TileBounds(cx, cy, 6).Max;
        }

        var tiles = new Dictionary<long, byte[]>();
        var expect = new List<TileExpectation>();

        for (int z = minZoom; z <= maxZoom; z++)
        {
            var (x0, y1) = MvtEncoder.TileXY(minLng, minLat, z);
            var (x1, y0) = MvtEncoder.TileXY(maxLng, maxLat, z);
            for (int x = Math.Min(x0, x1); x <= Math.Max(x0, x1); x++)
            {
                for (int y = Math.Min(y0, y1); y <= Math.Max(y0, y1); y++)
                {
                    var mvt = BuildTile(z, x, y);
                    long id = PmtilesWriter.TileId(z, x, y);
                    tiles[id] = mvt;
                    expect.Add(new TileExpectation
                    {
                        Z = z,
                        X = x,
                        Y = y,
                        TileId = id,
                        MvtSha256 = Sha256Hex(mvt),
                        MvtBytes = mvt.Length,
                    });
                }
            }
        }

        var center = MvtEncoder.ToLngLat(-58400, 33000);
        PmtilesWriter.Write(outPath, tiles, (10, center.Lng, center.Lat), minZoom, maxZoom,
            (minLng, minLat, maxLng, maxLat));

        var sidecar = new FixtureExpectation
        {
            Path = Path.GetFileName(outPath),
            MinZoom = minZoom,
            MaxZoom = maxZoom,
            Bounds = new[] { minLng, minLat, maxLng, maxLat },
            CenterZoom = 10,
            TileEntryCount = expect.Count,
            Tiles = expect,
        };
        var json = JsonSerializer.Serialize(sidecar, new JsonSerializerOptions
        {
            WriteIndented = true,
            PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        });
        File.WriteAllText(outPath + ".expect.json", json);

        Console.WriteLine($"fixture {outPath}: {expect.Count} tiles, {new FileInfo(outPath).Length} B, " +
                          $"large={large}, zoom {minZoom}-{maxZoom}");
        return 0;
    }

    private static byte[] BuildTile(int z, int x, int y)
    {
        // 几何直接使用瓦片内像素坐标（extent = 4096）；一条对角道路跨满整块瓦片。
        var layers = new Dictionary<string, List<MvtEncoder.Feature>>
        {
            ["road"] = new()
            {
                new MvtEncoder.Feature
                {
                    Type = 2,
                    Points = new[] { new[] { 200.0, 200.0, 3800.0, 3800.0 } },
                    Tags = new Dictionary<string, object>
                    {
                        ["kind"] = "asphalt",
                        ["speed_limit"] = 80,
                        ["one_way"] = "no",
                    },
                },
            },
            ["junction"] = new()
            {
                new MvtEncoder.Feature
                {
                    Type = 1,
                    Points = new[] { new[] { 2048.0, 2048.0 } },
                    Tags = new Dictionary<string, object> { ["prefab"] = "cross", ["movements"] = 4 },
                },
            },
            ["city"] = new()
            {
                new MvtEncoder.Feature
                {
                    Type = 1,
                    Points = new[] { new[] { 1024.0, 1024.0 } },
                    Tags = new Dictionary<string, object> { ["name"] = $"T{z}-{x}-{y}" },
                },
            },
            ["poi"] = new()
            {
                new MvtEncoder.Feature
                {
                    Type = 1,
                    Points = new[] { new[] { 3072.0, 3072.0 } },
                    Tags = new Dictionary<string, object> { ["type"] = "fuel", ["name"] = "depot" },
                },
            },
        };
        return MvtEncoder.EncodeTile(layers);
    }

    private static string Sha256Hex(byte[] data) => Convert.ToHexString(SHA256.HashData(data)).ToLowerInvariant();

    /// <summary>瓦片在 Web Mercator 下的经纬度范围（用于把夹具窗口收窄到单块瓦片）。</summary>
    private static ((double, double) Min, (double, double) Max) TileBounds(int x, int y, int z)
    {
        double n = Math.Pow(2, z);
        double lon0 = x / n * 360.0 - 180.0;
        double lon1 = (x + 1) / n * 360.0 - 180.0;
        static double LatOf(double ty, double n)
        {
            double m = Math.PI * (1.0 - 2.0 * ty / n);
            return Math.Atan(Math.Sinh(m)) * 180.0 / Math.PI;
        }
        double lat1 = LatOf(y, n), lat0 = LatOf(y + 1, n);
        return ((lon0, lat0), (lon1, lat1));
    }

    private sealed class FixtureExpectation
    {
        public string Path { get; set; } = "";
        public int MinZoom { get; set; }
        public int MaxZoom { get; set; }
        public double[] Bounds { get; set; } = [];
        public int CenterZoom { get; set; }
        public int TileEntryCount { get; set; }
        public List<TileExpectation> Tiles { get; set; } = [];
    }

    private sealed class TileExpectation
    {
        public int Z { get; set; }
        public int X { get; set; }
        public int Y { get; set; }
        public long TileId { get; set; }
        public string MvtSha256 { get; set; } = "";
        public int MvtBytes { get; set; }
    }
}
