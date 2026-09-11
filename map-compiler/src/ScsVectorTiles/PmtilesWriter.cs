using System.IO.Compression;
using System.Text;
using System.Text.Json;

namespace ScsVectorTiles;

/// <summary>
/// PMTiles v3 归档写入器（https://docs.protomaps.com/pmtiles/spec）。
///
/// 归档布局：header(127B) | root directory | metadata(JSON) | leaf directories | tile data。
///
/// v3 关键约束（本类逐项实现，2026-09 Batch 1.5 规范修复）：
/// <list type="bullet">
/// <item>tile_id 为 Hilbert 曲线上的累积位置，不是 zxy 行主序；</item>
/// <item>directory 是 varint 二进制结构（entry count / TileID 增量 / RunLength / Length / Offset），
///       与 metadata（JSON）是两种完全不同的编码；</item>
/// <item>offset 采用相对编码：与前一项数据连续时写 0，否则写 offset + 1；</item>
/// <item>header 127 字节 + root directory 必须落在归档前 16384 字节内，超限须拆 leaf directory；</item>
/// <item>compression 枚举为 0=Unknown / 1=None / 2=Gzip / 3=Brotli / 4=Zstd；</item>
/// <item>metadata 的 type 取 baselayer 或 overlay（矢量性由 tile_type 与 vector_layers 表达）。</item>
/// </list>
/// </summary>
public static class PmtilesWriter
{
    private const int HeaderSize = 127;

    /// <summary>root directory 允许的最大长度：header 起算，须落在归档前 16384 字节内。</summary>
    internal const int MaxRootDirLength = 16384 - HeaderSize;

    // PMTiles v3 Compression 枚举
    private const byte CompressionNone = 1;
    private const byte CompressionGzip = 2;

    // PMTiles v3 TileType 枚举
    private const byte TileTypeMvt = 1;

    /// <summary>directory 条目：偏移与长度均相对于 tile data 段（root 中的 leaf 指针则相对 leaf 段）。</summary>
    internal readonly record struct Entry(ulong TileId, ulong Offset, ulong Length, ulong RunLength);

    public static void Write(string path, Dictionary<long, byte[]> tiles,
        (int Z, double Lon, double Lat) center, int minZoom, int maxZoom,
        (double MinLon, double MinLat, double MaxLon, double MaxLat) bounds)
    {
        // ── tile data：按 tile_id 升序写入，每条 tile 独立 gzip 流
        // （gzip 尾在 Dispose 时写，单流连续写会截断）
        var ordered = tiles.OrderBy(t => t.Key).ToList();
        var tileData = new MemoryStream();
        var entries = new List<Entry>(ordered.Count);
        foreach (var (tileId, mvt) in ordered)
        {
            ulong start = (ulong)tileData.Position;
            using (var gzOut = new GZipStream(tileData, CompressionLevel.SmallestSize, leaveOpen: true))
            {
                gzOut.Write(mvt);
            }
            ulong length = (ulong)tileData.Position - start;
            entries.Add(new Entry((ulong)tileId, start, length, 1));
        }
        var tileDataBytes = tileData.ToArray();

        // ── 目录：优先全部放入 root；超出 16384 限制则拆一层 leaf directory
        var (rootBytes, leafBytes) = BuildDirectories(entries);

        // ── metadata（JSON；注意 directory 不是 JSON）
        var meta = JsonSerializer.Serialize(new
        {
            name = "ETS2Nav",
            type = "baselayer",
            vector_layers = new object[]
            {
                new { id = "road", fields = new Dictionary<string, string> { ["kind"] = "string", ["speed_limit"] = "number", ["one_way"] = "string" } },
                new { id = "junction", fields = new Dictionary<string, string> { ["prefab"] = "string", ["movements"] = "number" } },
                new { id = "city", fields = new Dictionary<string, string> { ["name"] = "string" } },
                new { id = "poi", fields = new Dictionary<string, string> { ["type"] = "string", ["name"] = "string" } },
            },
        });
        var metaBytes = Encoding.UTF8.GetBytes(meta);

        // ── 分段偏移
        ulong rootOffset = HeaderSize;
        ulong metaOffset = rootOffset + (ulong)rootBytes.Length;
        ulong leafOffset = metaOffset + (ulong)metaBytes.Length;
        ulong tileOffset = leafOffset + (ulong)leafBytes.Length;

        // ── 计数（全部写真实值；本实现不做 tile 去重，故每条 entry 的 run_length = 1）
        ulong addressedTiles = 0;
        ulong tileContents = 0;
        foreach (var e in entries)
        {
            addressedTiles += e.RunLength;
            tileContents += e.RunLength;
        }

        // ── clustered：tile data 顺序是否与目录顺序一致。
        // 本实现按 tile_id 升序写入，故偏移单调不减；此处显式校验后再置位，
        // 避免 flag 与真实 layout 不一致。
        bool clustered = true;
        for (int i = 1; i < entries.Count; i++)
        {
            if (entries[i].Offset < entries[i - 1].Offset + entries[i - 1].Length)
            {
                clustered = false;
                break;
            }
        }

        // ── header（127 字节）
        var h = new byte[HeaderSize];
        Encoding.ASCII.GetBytes("PMTiles").CopyTo(h, 0);
        h[7] = 3;                                                       // version
        WriteU64(h, 8, rootOffset);
        WriteU64(h, 16, (ulong)rootBytes.Length);                       // root_length
        WriteU64(h, 24, metaOffset);
        WriteU64(h, 32, (ulong)metaBytes.Length);                       // metadata_length
        WriteU64(h, 40, leafOffset);                                    // leaf_dirs_offset（空区段亦记录起点）
        WriteU64(h, 48, (ulong)leafBytes.Length);                       // leaf_dirs_length
        WriteU64(h, 56, tileOffset);
        WriteU64(h, 64, (ulong)tileDataBytes.Length);                   // tile_data_length
        WriteU64(h, 72, addressedTiles);
        WriteU64(h, 80, (ulong)entries.Count);                          // tile_entries_count
        WriteU64(h, 88, tileContents);
        h[96] = (byte)(clustered ? 1 : 0);
        h[97] = CompressionNone;    // internal：root/leaf/metadata 均未压缩
        h[98] = CompressionGzip;    // tile：每条 MVT 为独立 gzip 流
        h[99] = TileTypeMvt;
        h[100] = (byte)minZoom;
        h[101] = (byte)maxZoom;
        // bounds/center 为 int32 E7（度 × 1e7）；四舍五入而非截断，避免负值方向性偏差
        WriteI32(h, 102, E7(bounds.MinLon));
        WriteI32(h, 106, E7(bounds.MinLat));
        WriteI32(h, 110, E7(bounds.MaxLon));
        WriteI32(h, 114, E7(bounds.MaxLat));
        h[118] = (byte)center.Z;
        WriteI32(h, 119, E7(center.Lon));
        WriteI32(h, 123, E7(center.Lat));

        using var fs = File.Create(path);
        fs.Write(h);
        fs.Write(rootBytes);
        fs.Write(metaBytes);
        fs.Write(leafBytes);
        fs.Write(tileDataBytes);
    }

    /// <summary>
    /// 构建 root / leaf directory。
    /// 全部条目能放入 root 时 leaf 段为空；否则按有序条目均分为 2 的幂个 leaf，
    /// root 中每项为 leaf 指针（TileID = 该 leaf 首项 TileID，RunLength = 0，
    /// Offset/Length 指向 leaf 段）。只允许一层 leaf。
    /// </summary>
    internal static (byte[] Root, byte[] Leaves) BuildDirectories(IReadOnlyList<Entry> entries)
    {
        if (entries.Count == 0)
        {
            return (SerializeDirectory(entries), []);
        }

        for (int numLeaves = 1; ; numLeaves = numLeaves == 1 ? 2 : numLeaves * 2)
        {
            if (numLeaves == 1)
            {
                var only = SerializeDirectory(entries);
                if (only.Length <= MaxRootDirLength)
                {
                    return (only, []);
                }
                continue;
            }

            // 均分（保持 tile_id 升序；leaf 自身无长度上限，读取时按需抓取）
            int perLeaf = (entries.Count + numLeaves - 1) / numLeaves;
            var leaves = new List<(byte[] Bytes, ulong Offset)>();
            var rootEntries = new List<Entry>();
            ulong leafCursor = 0;
            for (int i = 0; i < entries.Count; i += perLeaf)
            {
                int count = Math.Min(perLeaf, entries.Count - i);
                var chunk = new List<Entry>(count);
                for (int j = 0; j < count; j++)
                {
                    chunk.Add(entries[i + j]);
                }
                var leafBytes = SerializeDirectory(chunk);
                rootEntries.Add(new Entry(chunk[0].TileId, leafCursor, (ulong)leafBytes.Length, 0));
                leaves.Add((leafBytes, leafCursor));
                leafCursor += (ulong)leafBytes.Length;
            }

            var rootBytes = SerializeDirectory(rootEntries);
            if (rootBytes.Length <= MaxRootDirLength)
            {
                var all = new MemoryStream();
                foreach (var (bytes, _) in leaves)
                {
                    all.Write(bytes);
                }
                return (rootBytes, all.ToArray());
            }

            if (numLeaves >= entries.Count)
            {
                // root 仅含 leaf 指针仍超限：说明条目数本身已不可能满足规范。
                // 宁可 fail-fast，也不生成违反规范的归档。
                throw new InvalidOperationException(
                    $"PMTiles root directory 无法压缩至 {MaxRootDirLength} 字节（{entries.Count} 条 entry，{numLeaves} 个 leaf）。");
            }
        }
    }

    /// <summary>
    /// PMTiles v3 directory 二进制编码：
    /// entry count | TileID 增量 | RunLength | Length | Offset（相对编码），全部 unsigned varint。
    /// </summary>
    internal static byte[] SerializeDirectory(IReadOnlyList<Entry> entries)
    {
        var ms = new MemoryStream();
        WriteUvarint(ms, (ulong)entries.Count);

        ulong lastId = 0;
        foreach (var e in entries)
        {
            WriteUvarint(ms, e.TileId - lastId);
            lastId = e.TileId;
        }
        foreach (var e in entries)
        {
            WriteUvarint(ms, e.RunLength);
        }
        foreach (var e in entries)
        {
            WriteUvarint(ms, e.Length);
        }
        for (int i = 0; i < entries.Count; i++)
        {
            // 与前一项目标数据连续 -> 0；否则 offset + 1（decoder 侧 0 表示
            // previous.offset + previous.length，+1 用于区分「偏移恰为 0 的首项」）
            if (i > 0 && entries[i].Offset == entries[i - 1].Offset + entries[i - 1].Length)
            {
                WriteUvarint(ms, 0);
            }
            else
            {
                WriteUvarint(ms, entries[i].Offset + 1);
            }
        }
        return ms.ToArray();
    }

    /// <summary>
    /// z/x/y → PMTiles v3 tile_id：Hilbert 曲线上的累积位置。
    /// tile_id = (4^z - 1) / 3 + hilbert_d(z, x, y)。
    /// </summary>
    public static long TileId(int z, int x, int y)
    {
        long baseId = ((1L << (2 * z)) - 1) / 3;
        return baseId + HilbertDistance(z, x, y);
    }

    /// <summary>d 维（z 阶）Hilbert 曲线上 (x, y) 的序号。</summary>
    internal static long HilbertDistance(int z, int x, int y)
    {
        long d = 0;
        int n = 1 << z;
        for (int s = n >> 1; s > 0; s >>= 1)
        {
            int rx = (x & s) > 0 ? 1 : 0;
            int ry = (y & s) > 0 ? 1 : 0;
            d += (long)s * s * ((3 * rx) ^ ry);
            if (ry == 0)
            {
                if (rx == 1)
                {
                    x = n - 1 - x;
                    y = n - 1 - y;
                }
                (x, y) = (y, x);
            }
        }
        return d;
    }

    private static int E7(double degrees) => (int)Math.Round(degrees * 1e7, MidpointRounding.AwayFromZero);

    private static void WriteUvarint(Stream s, ulong v)
    {
        while (v >= 0x80)
        {
            s.WriteByte((byte)(v | 0x80));
            v >>= 7;
        }
        s.WriteByte((byte)v);
    }

    private static void WriteU64(byte[] b, int off, ulong v)
    {
        for (int i = 0; i < 8; i++) b[off + i] = (byte)(v >> (8 * i));
    }

    private static void WriteI32(byte[] b, int off, int v)
    {
        for (int i = 0; i < 4; i++) b[off + i] = (byte)(v >> (8 * i));
    }
}
