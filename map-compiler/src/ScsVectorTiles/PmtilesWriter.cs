using System.IO.Compression;
using System.Text;
using System.Text.Json;

namespace ScsVectorTiles;

/// <summary>
/// PMTiles v3 归档写入器（pmtiles.org 规范）：header(127B) + root directory(JSON) + gzip MVT tiles。
/// tile_id 用 zxy 线性编码：tile_id = ((1 &lt;&lt; (2*z)) - 1) / 3 + (y &lt;&lt; z) + x。
/// </summary>
public static class PmtilesWriter
{
    public static void Write(string path, Dictionary<long, byte[]> tiles,
        (int Z, double Lon, double Lat) center, int minZoom, int maxZoom,
        (double MinLon, double MinLat, double MaxLon, double MaxLat) bounds)
    {
        // tile 数据（每条 tile 独立 gzip 流——gzip 尾在 Dispose 时写，单流连续写会截断）
        var tileData = new MemoryStream();
        var ordered = tiles.OrderBy(t => t.Key).ToList();
        var entryList = new List<(long, long, long, uint)>();
        foreach (var (tileId, mvt) in ordered)
        {
            long start = tileData.Position;
            using (var gzOut = new GZipStream(tileData, CompressionLevel.SmallestSize, leaveOpen: true))
            {
                gzOut.Write(mvt);
            }
            long end = tileData.Position;
            entryList.Add((tileId, start, end - start, 1));
        }
        var tileDataBytes = tileData.ToArray();

        // root directory（JSON）
        var dir = new Dictionary<string, object>
        {
            ["rootDir"] = new
            {
                tiles = entryList.Select(e => new
                {
                    tile_id = e.Item1,
                    offset = e.Item2,
                    length = e.Item3,
                    run_length = e.Item4,
                }).ToList(),
            },
        };
        var dirJson = JsonSerializer.Serialize(dir);
        var dirBytes = Encoding.UTF8.GetBytes(dirJson);

        // metadata（JSON）
        var meta = JsonSerializer.Serialize(new
        {
            name = "ETS2Nav",
            type = "vector",
            vector_layers = new object[]
            {
                new { id = "road", fields = new Dictionary<string, string> { ["kind"] = "string", ["speed_limit"] = "number" } },
                new { id = "junction", fields = new Dictionary<string, string> { ["prefab"] = "string", ["movements"] = "number" } },
                new { id = "city", fields = new Dictionary<string, string> { ["name"] = "string" } },
                new { id = "poi", fields = new Dictionary<string, string> { ["type"] = "string", ["name"] = "string" } },
            },
        });
        var metaBytes = Encoding.UTF8.GetBytes(meta);

        // header（127 字节）
        var h = new byte[127];
        Encoding.ASCII.GetBytes("PMTiles").CopyTo(h, 0);
        h[7] = 3;   // version
        WriteU64(h, 8, 127);                                                   // root_offset（header 后）
        WriteU64(h, 16, (ulong)dirBytes.Length);                          // root_length
        WriteU64(h, 24, 127 + (ulong)dirBytes.Length);                    // metadata_offset（root 后）
        WriteU64(h, 32, (ulong)metaBytes.Length);                         // metadata_length
        WriteU64(h, 40, 0);                                              // leaf_dirs_offset
        WriteU64(h, 48, 0);                                              // leaf_dirs_length
        WriteU64(h, 56, 127 + (ulong)(dirBytes.Length + metaBytes.Length)); // tile_data_offset
        WriteU64(h, 64, (ulong)tileDataBytes.Length);                    // tile_data_length
        WriteU64(h, 72, 0);                                              // addressed_tiles_count
        WriteU64(h, 80, (ulong)entryList.Count);                         // tile_entries_count
        WriteU64(h, 88, (ulong)entryList.Count);                         // tile_contents_count
        h[96] = 0;                                                       // clustered = false
        h[97] = 0;                                                       // internal_compression = none
        h[98] = 1;                                                       // tile_compression = gzip
        h[99] = 1;                                                       // tile_type = MVT
        h[100] = (byte)minZoom;
        h[101] = (byte)maxZoom;
        WriteI32(h, 102, (int)(bounds.MinLon * 1e7));
        WriteI32(h, 106, (int)(bounds.MinLat * 1e7));
        WriteI32(h, 110, (int)(bounds.MaxLon * 1e7));
        WriteI32(h, 114, (int)(bounds.MaxLat * 1e7));
        h[118] = (byte)center.Z;
        WriteI32(h, 119, (int)(center.Lon * 1e7));
        WriteI32(h, 123, (int)(center.Lat * 1e7));

        using var fs = File.Create(path);
        fs.Write(h);
        fs.Write(dirBytes);
        fs.Write(metaBytes);
        fs.Write(tileDataBytes);
    }

    /// <summary>zxy → PMTiles tile_id（zxy 线性编码）。</summary>
    public static long TileId(int z, int x, int y)
        => ((1L << (2 * z)) - 1) / 3 + ((long)y << z) + x;

    private static void WriteU64(byte[] b, int off, ulong v)
    {
        for (int i = 0; i < 8; i++) b[off + i] = (byte)(v >> (8 * i));
    }

    private static void WriteI32(byte[] b, int off, int v)
    {
        for (int i = 0; i < 4; i++) b[off + i] = (byte)(v >> (8 * i));
    }
}
