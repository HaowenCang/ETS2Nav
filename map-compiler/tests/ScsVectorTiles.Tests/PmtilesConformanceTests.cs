using System.IO.Compression;
using System.Text;
using ScsVectorTiles;
using Xunit;

namespace ScsVectorTiles.Tests;

/// <summary>
/// PMTiles v3 规范符合性测试（P4R Batch 1.5）。
///
/// 读取侧刻意不复用 writer 的任何代码：<see cref="SpecReader"/> 是按 PMTiles v3
/// 规范文本独立实现的解码器，用于避免「自己的 encoder 被自己的 decoder 读回」这种
/// 自证式测试。真正的跨实现互操作验证由 Node + 官方 pmtiles reader 承担
/// （tools/ets2nav-web/scripts/verify-pmtiles.mjs）。
/// </summary>
public class PmtilesConformanceTests
{
    private const int HeaderSize = 127;
    private const int RootLimit = 16384; // root 必须落在归档前 16384 字节内

    private static string TempFile(string name) =>
        Path.Combine(Path.GetTempPath(), $"p4r15-{Guid.NewGuid():N}-{name}.pmtiles");

    /// <summary>构造一条非平凡但可预测的 MVT 载荷。</summary>
    private static byte[] FakeMvt(int z, int x, int y)
    {
        var payload = new List<byte> { 0x1A, 0x0A }; // 任意字节，内容不参与 MVT 解析
        payload.AddRange(Encoding.ASCII.GetBytes($"mvt:{z}/{x}/{y}"));
        return payload.ToArray();
    }

    private static Dictionary<long, byte[]> WriteTiles(
        (int Z, int X, int Y)[] coords, string path,
        int minZoom = 6, int maxZoom = 11,
        (double, double, double, double)? bounds = null)
    {
        var tiles = new Dictionary<long, byte[]>();
        foreach (var (z, x, y) in coords)
        {
            tiles[PmtilesWriter.TileId(z, x, y)] = FakeMvt(z, x, y);
        }
        PmtilesWriter.Write(path, tiles, (maxZoom, 0.0, 0.0), minZoom, maxZoom,
            bounds ?? (-1.0, -1.0, 1.0, 1.0));
        return tiles;
    }

    // ── T2：TileID 参考向量（PMTiles v3 规范给出的官方样例）────────────────

    [Theory]
    // z, x, y, expected tile_id —— 规范 reference cases，不得为迁就实现而修改
    [InlineData(0, 0, 0, 0)]
    [InlineData(1, 0, 0, 1)]
    [InlineData(1, 0, 1, 2)]
    [InlineData(1, 1, 1, 3)]
    [InlineData(1, 1, 0, 4)]
    [InlineData(2, 0, 0, 5)]
    [InlineData(12, 3423, 1763, 19078479)]
    public void T2_TileId_MatchesSpecReferenceVectors(int z, int x, int y, long expected)
    {
        Assert.Equal(expected, PmtilesWriter.TileId(z, x, y));
    }

    [Fact]
    public void T2_TileId_IsBijectiveWithinZoom()
    {
        // 同一 zoom 内必须是一一映射，且恰好覆盖该层区间
        const int z = 5;
        int n = 1 << z;
        long baseId = ((1L << (2 * z)) - 1) / 3;
        var seen = new HashSet<long>();
        for (int x = 0; x < n; x++)
        {
            for (int y = 0; y < n; y++)
            {
                long id = PmtilesWriter.TileId(z, x, y);
                Assert.InRange(id, baseId, baseId + (long)n * n - 1);
                Assert.True(seen.Add(id), $"tile_id 冲突: z={z} x={x} y={y} -> {id}");
            }
        }
        Assert.Equal(n * n, seen.Count);
    }

    [Fact]
    public void T2_TileId_ZoomBasesAreContiguous()
    {
        // 各 zoom 的起始 id 必须首尾相接（累积位置语义）
        for (int z = 0; z < 12; z++)
        {
            long count = 1L << (2 * z);
            long nextBase = ((1L << (2 * (z + 1))) - 1) / 3;
            Assert.Equal(nextBase, PmtilesWriter.TileId(z, 0, 0) + count);
        }
    }

    // ── T1：header ─────────────────────────────────────────────────────────

    [Fact]
    public void T1_Header_FieldsAreSpecConformant()
    {
        var path = TempFile("header");
        try
        {
            var coords = new[] { (6, 33, 21), (10, 543, 351) };
            WriteTiles(coords, path, minZoom: 6, maxZoom: 11,
                bounds: (5.5, 47.0, 15.5, 55.5));
            var f = File.ReadAllBytes(path);

            Assert.Equal("PMTiles", Encoding.ASCII.GetString(f, 0, 7));
            Assert.Equal(3, f[7]);

            ulong rootOff = U64(f, 8), rootLen = U64(f, 16);
            ulong metaOff = U64(f, 24), metaLen = U64(f, 32);
            ulong leafOff = U64(f, 40), leafLen = U64(f, 48);
            ulong dataOff = U64(f, 56), dataLen = U64(f, 64);

            Assert.Equal((ulong)HeaderSize, rootOff);
            Assert.True(rootLen > 0);
            Assert.Equal(rootOff + rootLen, metaOff);
            Assert.True(metaLen > 0);
            Assert.Equal(metaOff + metaLen, leafOff);
            Assert.Equal(leafOff + leafLen, dataOff);
            Assert.Equal((ulong)f.Length, dataOff + dataLen);

            // Compression 枚举：internal = 1(None)，tile = 2(Gzip)
            Assert.Equal(1, f[97]);
            Assert.Equal(2, f[98]);
            // TileType：1 = MVT
            Assert.Equal(1, f[99]);
            Assert.Equal(6, f[100]);
            Assert.Equal(11, f[101]);

            Assert.Equal(55_000_000, I32(f, 102));  // minLon  E7
            Assert.Equal(470_000_000, I32(f, 106)); // minLat
            Assert.Equal(155_000_000, I32(f, 110)); // maxLon
            Assert.Equal(555_000_000, I32(f, 114)); // maxLat
            Assert.Equal(11, f[118]);               // center zoom
            Assert.Equal(0, I32(f, 119));
            Assert.Equal(0, I32(f, 123));
        }
        finally { File.Delete(path); }
    }

    [Fact]
    public void T1_Header_InternalCompressionMatchesActualDirectoryBytes()
    {
        // header 声明与真实内容必须一致：internal=None 时 root 必须是可解析的裸 varint
        var path = TempFile("compression");
        try
        {
            WriteTiles(new[] { (10, 543, 351) }, path);
            var f = File.ReadAllBytes(path);
            Assert.Equal(1, f[97]);
            Assert.NotEqual(0x1f, f[HeaderSize]); // 不是 gzip magic，确为未压缩

            ulong rootLen = U64(f, 16);
            var root = SpecReader.ReadDirectory(f, (int)U64(f, 8), (int)rootLen);
            Assert.Single(root);
        }
        finally { File.Delete(path); }
    }

    // ── T3：directory 二进制编码 ────────────────────────────────────────────

    [Fact]
    public void T3_RootDirectory_IsBinaryVarintNotJson()
    {
        var path = TempFile("dirbinary");
        try
        {
            WriteTiles(new[] { (6, 33, 21), (10, 543, 351), (11, 1087, 703) }, path);
            var f = File.ReadAllBytes(path);
            ulong rootLen = U64(f, 16);

            // JSON 回归守卫：v3 directory 不得以 '{' 开头
            Assert.NotEqual((byte)'{', f[HeaderSize]);
            Assert.NotEqual(0xEF, f[HeaderSize]); // 也不得是 UTF-8 BOM

            var entries = SpecReader.ReadDirectory(f, HeaderSize, (int)rootLen);
            Assert.Equal(3, entries.Count);
            // 规范要求 entry 按 tile_id 升序
            for (int i = 1; i < entries.Count; i++)
            {
                Assert.True(entries[i].TileId > entries[i - 1].TileId);
            }
            // 无去重：每条 run_length = 1
            Assert.All(entries, e => Assert.Equal(1UL, e.RunLength));

            // 无 leaf 时 root 条目必须直接指向 tile data 段且拼接后等于 tile_data_length
            ulong dataLen = U64(f, 64);
            Assert.Equal(dataLen, entries.Aggregate(0UL, (acc, e) => acc + e.Length));
        }
        finally { File.Delete(path); }
    }

    [Fact]
    public void T3_HeaderCounts_MatchDirectoryContents()
    {
        var path = TempFile("counts");
        try
        {
            WriteTiles(new[] { (6, 33, 21), (7, 67, 43), (10, 543, 351) }, path);
            var f = File.ReadAllBytes(path);
            var entries = SpecReader.ReadDirectory(f, HeaderSize, (int)U64(f, 16));

            Assert.Equal((ulong)entries.Count, U64(f, 80));            // tile_entries_count
            Assert.Equal(entries.Aggregate(0UL, (acc, e) => acc + e.RunLength), U64(f, 72)); // addressed_tiles_count
            Assert.Equal(3UL, U64(f, 80));
            Assert.Equal(3UL, U64(f, 72));
            // 本实现无去重：contents == addressed
            Assert.Equal(U64(f, 72), U64(f, 88));
        }
        finally { File.Delete(path); }
    }

    [Fact]
    public void T3_DirectoryOffsetEncoding_IsRelative()
    {
        // 数据按 tile_id 升序连续写入时，第二条起的 offset 必须编码为 0；
        // 解码侧 0 表示 previous.offset + previous.length。
        var path = TempFile("offsets");
        try
        {
            var coords = new[] { (6, 33, 21), (7, 67, 43), (8, 135, 87), (9, 271, 175) };
            WriteTiles(coords, path);
            var f = File.ReadAllBytes(path);
            int rootStart = HeaderSize;
            int rootLen = (int)U64(f, 16);

            // 独立解码 offset 段，统计 0 的个数（= 连续条目数）
            var raw = SpecReader.ReadRawOffsetVarints(f, rootStart, rootLen, out int entryCount);
            Assert.Equal(4, entryCount);
            Assert.Equal(3, raw.Count(v => v == 0));
            Assert.True(raw[0] > 0); // 首项写 offset + 1

            var entries = SpecReader.ReadDirectory(f, rootStart, rootLen);
            ulong expect = 0;
            foreach (var e in entries)
            {
                Assert.Equal(expect, e.Offset);
                expect += e.Length;
            }
        }
        finally { File.Delete(path); }
    }

    // ── T4：tile 往返 ──────────────────────────────────────────────────────

    [Fact]
    public void T4_TilesRoundTrip_WithGzipAndPreciseBytes()
    {
        var path = TempFile("roundtrip");
        try
        {
            var coords = new[] { (6, 33, 21), (8, 135, 87), (10, 543, 351), (11, 1087, 703) };
            WriteTiles(coords, path);
            var f = File.ReadAllBytes(path);
            var reader = new SpecReader(f);

            foreach (var (z, x, y) in coords)
            {
                var stored = reader.GetTile(z, x, y);
                Assert.NotNull(stored);
                // tile_compression = gzip，故存储字节必须是 gzip 且解压后等于原始 MVT
                Assert.Equal(0x1f, stored![0]);
                Assert.Equal(0x8b, stored[1]);
                Assert.Equal(FakeMvt(z, x, y), Gunzip(stored));
            }
        }
        finally { File.Delete(path); }
    }

    [Fact]
    public void T4_MissingTile_ReturnsNull()
    {
        var path = TempFile("missing");
        try
        {
            WriteTiles(new[] { (10, 543, 351) }, path);
            var reader = new SpecReader(File.ReadAllBytes(path));
            Assert.NotNull(reader.GetTile(10, 543, 351));
            Assert.Null(reader.GetTile(10, 544, 351));
        }
        finally { File.Delete(path); }
    }

    // ── T5：超出 root 限制时的 leaf directory ───────────────────────────────

    [Fact]
    public void T5_LargeArchive_SplitsIntoLeafDirectoriesAndStaysReadable()
    {
        var path = TempFile("leaf");
        try
        {
            // 规模需足以让单层 root 超过 16384 字节限制
            var coords = new List<(int, int, int)>();
            for (int x = 0; x < 81; x++)
            {
                for (int y = 0; y < 81; y++)
                {
                    coords.Add((10, x, y));
                }
            }
            Assert.True(coords.Count > 6000, "测试规模不足以保证触发 leaf 拆分");
            WriteTiles(coords.ToArray(), path);

            var f = File.ReadAllBytes(path);
            ulong rootOff = U64(f, 8), rootLen = U64(f, 16);
            ulong leafOff = U64(f, 40), leafLen = U64(f, 48);

            // 硬约束：header 起算 root 必须落在前 16384 字节内
            Assert.True(rootOff + rootLen <= RootLimit,
                $"root 超出限制: rootOff={rootOff} rootLen={rootLen}");
            Assert.True(leafLen > 0, "超出 root 限制时必须生成 leaf directory");

            var root = SpecReader.ReadDirectory(f, (int)rootOff, (int)rootLen);
            Assert.True(root.Count > 1, "root 应仅保留 leaf 指针");
            Assert.All(root, e => Assert.Equal(0UL, e.RunLength)); // leaf 指针 run_length = 0

            var reader = new SpecReader(f);
            // 全量校验：每块 tile 都必须可定位并解压回原值
            int checkedCount = 0;
            foreach (var (z, x, y) in coords)
            {
                var stored = reader.GetTile(z, x, y);
                Assert.NotNull(stored);
                Assert.Equal(FakeMvt(z, x, y), Gunzip(stored!));
                checkedCount++;
            }
            Assert.Equal(coords.Count, checkedCount);
        }
        finally { File.Delete(path); }
    }

    [Fact]
    public void T5_LeafPointersAreOrderedAndCoverAllEntries()
    {
        var path = TempFile("leaforder");
        try
        {
            var coords = new List<(int, int, int)>();
            for (int x = 0; x < 81; x++)
            {
                for (int y = 0; y < 81; y++)
                {
                    coords.Add((10, x, y));
                }
            }
            WriteTiles(coords.ToArray(), path);
            var f = File.ReadAllBytes(path);
            var root = SpecReader.ReadDirectory(f, (int)U64(f, 8), (int)U64(f, 16));

            for (int i = 1; i < root.Count; i++)
            {
                Assert.True(root[i].TileId > root[i - 1].TileId, "leaf 指针必须按 tile_id 升序");
            }
            // leaf 段内偏移必须连续覆盖整个 leaf 区段
            ulong expect = 0;
            foreach (var e in root)
            {
                Assert.Equal(expect, e.Offset);
                expect += e.Length;
            }
            Assert.Equal(U64(f, 48), expect);
        }
        finally { File.Delete(path); }
    }

    // ── T6：MVT Value 编码（2026-09 修复的浏览器渲染缺陷回归守卫）───────────

    [Fact]
    public void T6_IntegerTagUsesIntValueField4()
    {
        // 回归：整数原先写为 field 2（float_value 的字段号）却用 varint 线型，
        // MapLibre 解析时静默丢弃整个要素——带 speed_limit 的 road 与带 movements 的
        // junction 在浏览器中完全不渲染。规范中 int_value 为 field 4。
        var bytes = MvtEncoder.EncodeValue(80);
        Assert.Equal(new byte[] { 0x20, 0x50 }, bytes); // tag=(4<<3)|0，varint 80
        Assert.Equal(4, bytes[0] >> 3);
        Assert.Equal(0, bytes[0] & 7);
    }

    [Fact]
    public void T6_IntegerTagIsNotEncodedAsFloatValue()
    {
        // 显式锁定不得回退到 field 2（float_value 的字段号取值为 2、线型必须为 5）
        var bytes = MvtEncoder.EncodeValue(80);
        Assert.NotEqual(2, bytes[0] >> 3);
        Assert.NotEqual(5, bytes[0] & 7);
    }

    [Fact]
    public void T6_NegativeIntegerRoundTripsAsInt64Varint()
    {
        var bytes = MvtEncoder.EncodeValue(-1);
        Assert.Equal(4, bytes[0] >> 3);
        var payload = bytes.AsSpan(1);
        Assert.Equal(10, payload.Length);                 // int64 的 -1 为 10 字节
        Assert.All(payload[..9].ToArray(), b => Assert.Equal(0xFF, b));
        Assert.Equal(0x01, payload[9]);
    }

    [Fact]
    public void T6_StringValueUsesField1()
    {
        var bytes = MvtEncoder.EncodeValue("depot");
        Assert.Equal(1, bytes[0] >> 3);
        Assert.Equal(2, bytes[0] & 7);
        Assert.Equal(5, bytes[1]);
        Assert.Equal("depot", Encoding.UTF8.GetString(bytes, 2, 5));
    }

    [Fact]
    public void T6_DoubleValueUsesField3Fixed64()
    {
        var bytes = MvtEncoder.EncodeValue(1.5);
        Assert.Equal(3, bytes[0] >> 3);
        Assert.Equal(1, bytes[0] & 7);                    // fixed64
        Assert.Equal(9, bytes.Length);                    // 1 tag + 8 字节
        Assert.Equal(1.5, BitConverter.ToDouble(bytes, 1));
    }

    [Fact]
    public void T6_UnsupportedTagTypeThrowsInsteadOfWritingEmptyValue()
    {
        // 旧实现遇到未知类型写出零字节 Value（静默损坏）；现要求显式失败。
        var ex = Assert.Throws<NotSupportedException>(() => MvtEncoder.EncodeValue(true));
        Assert.Contains("Boolean", ex.Message);
    }

    // ── T7：MVT 几何段语义（TileBuilder 道路编码回归守卫）──────────────────

    [Fact]
    public void T7_TwoPointLineInSingleSegmentEmitsMoveToAndLineTo()
    {
        // 契约：Feature.Points 的每个元素是一串扁平 x,y,x,y… 坐标；
        // 线段两端必须位于同一段内，否则只发出 MoveTo 而不发出 LineTo。
        var f = new MvtEncoder.Feature
        {
            Type = 2,
            Points = new[] { new[] { 200.0, 200.0, 3800.0, 3800.0 } },
            Tags = new Dictionary<string, object> { ["kind"] = "asphalt" },
        };
        var geom = DecodeVarints(MvtEncoder.EncodeGeometry(f));

        Assert.Equal(new List<uint> { 9, 400, 400, 10, 7200, 7200 }, geom);
        Assert.Equal(1u, geom[0] & 0x7u);   // MoveTo
        Assert.Equal(2u, geom[3] & 0x7u);   // LineTo —— 缺此则线退化为孤立点
    }

    [Fact]
    public void T7_TwoSeparateSegmentsDegenerateToTwoMoveTo()
    {
        // 反面守卫：把两端拆成两个「段」只会得到两个 MoveTo，线不成立。
        // 这是 2026-09 修复前 TileBuilder 的写法，记录在案以防回退。
        var f = new MvtEncoder.Feature
        {
            Type = 2,
            Points = new[] { new[] { 200.0, 200.0 }, new[] { 3800.0, 3800.0 } },
            Tags = new Dictionary<string, object> { ["kind"] = "asphalt" },
        };
        var geom = DecodeVarints(MvtEncoder.EncodeGeometry(f));

        Assert.Equal(new List<uint> { 9, 400, 400, 9, 7200, 7200 }, geom);
        Assert.DoesNotContain(geom, c => (c & 0x7) == 2); // 无任何 LineTo
    }

    [Fact]
    public void T7_PointGeometryEmitsOnlyMoveTo()
    {
        var f = new MvtEncoder.Feature
        {
            Type = 1,
            Points = new[] { new[] { 2048.0, 2048.0 } },
            Tags = new Dictionary<string, object> { ["type"] = "fuel" },
        };
        Assert.Equal(new List<uint> { 9, 4096, 4096 }, DecodeVarints(MvtEncoder.EncodeGeometry(f)));
    }

    [Fact]
    public void T7_MultiVertexLineEmitsSingleLineToWithCount()
    {
        var f = new MvtEncoder.Feature
        {
            Type = 2,
            Points = new[] { new[] { 0.0, 0.0, 100.0, 0.0, 100.0, 100.0 } },
            Tags = new Dictionary<string, object> { ["kind"] = "asphalt" },
        };
        var geom = DecodeVarints(MvtEncoder.EncodeGeometry(f));
        // MoveTo(0,0) + LineTo(count=2)：(0,0) → (100,0) → (100,100)
        Assert.Equal(9u, geom[0]);          // 命令整数 (1 | 1<<3)
        Assert.Equal(1u, geom[0] & 0x7u);   // MoveTo
        Assert.Equal(18u, geom[3]);         // 命令整数 (2 | 2<<3)：两个后续顶点
        Assert.Equal(2u, geom[3] & 0x7u);   // LineTo
        Assert.Equal(2u, geom[3] >> 3);
        Assert.Equal(new List<uint> { 9, 0, 0, 18, 200, 0, 0, 200 }, geom);
        Assert.Equal(8, geom.Count);        // 3 + (1 + 2*2)
    }

    /// <summary>解码一串 packed unsigned varint（仅供测试）。</summary>
    private static List<uint> DecodeVarints(List<byte> bytes)
    {
        var result = new List<uint>();
        int pos = 0;
        while (pos < bytes.Count)
        {
            uint r = 0; int shift = 0; byte b;
            do { b = bytes[pos++]; r |= (uint)(b & 0x7F) << shift; shift += 7; } while ((b & 0x80) != 0);
            result.Add(r);
        }
        return result;
    }

    // ── metadata ──────────────────────────────────────────────────────────
    [Fact]
    public void Metadata_TypeIsBaselayerAndParsesAsJson()
    {
        var path = TempFile("meta");
        try
        {
            WriteTiles(new[] { (10, 543, 351) }, path);
            var f = File.ReadAllBytes(path);
            var json = Encoding.UTF8.GetString(f, (int)U64(f, 24), (int)U64(f, 32));
            using var doc = System.Text.Json.JsonDocument.Parse(json); // 必须可解析
            var root = doc.RootElement;

            Assert.Equal("baselayer", root.GetProperty("type").GetString());
            Assert.Equal("ETS2Nav", root.GetProperty("name").GetString());
            var layers = root.GetProperty("vector_layers").EnumerateArray()
                .Select(l => l.GetProperty("id").GetString()).ToList();
            Assert.Equal(new[] { "road", "junction", "city", "poi" }, layers);
        }
        finally { File.Delete(path); }
    }

    // ── helpers ───────────────────────────────────────────────────────────

    private static byte[] Gunzip(byte[] data)
    {
        using var input = new MemoryStream(data);
        using var gz = new GZipStream(input, CompressionMode.Decompress);
        using var output = new MemoryStream();
        gz.CopyTo(output);
        return output.ToArray();
    }

    private static ulong U64(byte[] b, int off)
    {
        ulong v = 0;
        for (int i = 0; i < 8; i++) v |= (ulong)b[off + i] << (8 * i);
        return v;
    }

    private static int I32(byte[] b, int off)
    {
        int v = 0;
        for (int i = 0; i < 4; i++) v |= b[off + i] << (8 * i);
        return v;
    }

    // ── 独立 PMTiles v3 读取器（按规范文本实现，不复用 writer 代码）────────

    internal sealed record DirEntry(ulong TileId, ulong Offset, ulong Length, ulong RunLength);

    internal sealed class SpecReader
    {
        private readonly byte[] _f;
        private readonly ulong _rootOff, _rootLen, _leafOff, _leafLen, _dataOff;
        private List<DirEntry>? _root;

        public SpecReader(byte[] file)
        {
            _f = file;
            Assert.Equal("PMTiles", Encoding.ASCII.GetString(file, 0, 7));
            Assert.Equal(3, file[7]);
            _rootOff = U64(file, 8);
            _rootLen = U64(file, 16);
            _leafOff = U64(file, 40);
            _leafLen = U64(file, 48);
            _dataOff = U64(file, 56);
        }

        public byte[]? GetTile(int z, int x, int y)
        {
            ulong target = (ulong)PmtilesWriter.TileId(z, x, y);
            _root ??= ReadDirectory(_f, (int)_rootOff, (int)_rootLen);

            var hit = FindEntry(_root, target);
            if (hit is null) return null;
            if (hit.RunLength == 0)
            {
                // leaf 指针：offset/length 相对 leaf 段
                var leaf = ReadDirectory(_f, (int)(_leafOff + hit.Offset), (int)hit.Length);
                hit = FindEntry(leaf, target);
                if (hit is null) return null;
            }
            // 规范要求：条目覆盖 [TileId, TileId + RunLength)。无覆盖即「无此瓦片」，
            // 不得把前一条目的数据当作命中返回。
            if (target < hit.TileId || target >= hit.TileId + hit.RunLength) return null;
            int start = (int)(_dataOff + hit.Offset);
            return _f.AsSpan(start, (int)hit.Length).ToArray();
        }

        /// <summary>目录中 tile_id 不大于 target 的最后一项（规范规定的二分查找语义）。</summary>
        private static DirEntry? FindEntry(List<DirEntry> entries, ulong target)
        {
            int lo = 0, hi = entries.Count - 1;
            while (lo <= hi)
            {
                int mid = (hi + lo) >> 1;
                if (target > entries[mid].TileId) lo = mid + 1;
                else if (target < entries[mid].TileId) hi = mid - 1;
                else return entries[mid];
            }
            return hi >= 0 && (entries[hi].RunLength == 0 || target - entries[hi].TileId < entries[hi].RunLength)
                ? entries[hi]
                : null;
        }

        public static List<DirEntry> ReadDirectory(byte[] buf, int off, int len)
        {
            int pos = off;
            int end = off + len;
            ulong count = Uvarint(buf, ref pos);
            var ids = new ulong[count];
            ulong last = 0;
            for (ulong i = 0; i < count; i++)
            {
                last += Uvarint(buf, ref pos);
                ids[i] = last;
            }
            var runs = new ulong[count];
            for (ulong i = 0; i < count; i++) runs[i] = Uvarint(buf, ref pos);
            var lengths = new ulong[count];
            for (ulong i = 0; i < count; i++) lengths[i] = Uvarint(buf, ref pos);
            var offsets = new ulong[count];
            for (ulong i = 0; i < count; i++)
            {
                ulong v = Uvarint(buf, ref pos);
                offsets[i] = v == 0 ? offsets[i - 1] + lengths[i - 1] : v - 1;
            }
            Assert.True(pos <= end, "directory 解码越界——长度字段与内容不一致");

            var result = new List<DirEntry>((int)count);
            for (ulong i = 0; i < count; i++)
            {
                result.Add(new DirEntry(ids[i], offsets[i], lengths[i], runs[i]));
            }
            return result;
        }

        /// <summary>仅解码 offset 段，用于断言相对编码语义。</summary>
        public static List<ulong> ReadRawOffsetVarints(byte[] buf, int off, int len, out int entryCount)
        {
            int pos = off;
            ulong count = Uvarint(buf, ref pos);
            entryCount = (int)count;
            for (ulong i = 0; i < count; i++) Uvarint(buf, ref pos); // ids
            for (ulong i = 0; i < count; i++) Uvarint(buf, ref pos); // runs
            for (ulong i = 0; i < count; i++) Uvarint(buf, ref pos); // lengths
            var offsets = new List<ulong>((int)count);
            for (ulong i = 0; i < count; i++) offsets.Add(Uvarint(buf, ref pos));
            return offsets;
        }

        private static ulong Uvarint(byte[] buf, ref int pos)
        {
            ulong result = 0;
            int shift = 0;
            while (true)
            {
                byte b = buf[pos++];
                result |= (ulong)(b & 0x7F) << shift;
                if ((b & 0x80) == 0) return result;
                shift += 7;
                Assert.True(shift <= 63, "varint 超过 10 字节——编码非法");
            }
        }

        private static ulong U64(byte[] b, int off)
        {
            ulong v = 0;
            for (int i = 0; i < 8; i++) v |= (ulong)b[off + i] << (8 * i);
            return v;
        }
    }
}
