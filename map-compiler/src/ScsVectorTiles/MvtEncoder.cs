using System.Buffers.Binary;

namespace ScsVectorTiles;

/// <summary>
/// Mapbox Vector Tile（MVT）编码器——零依赖手写 protobuf 编码。
/// 坐标系：游戏坐标 (x, z) 米 → 经纬度（lng = x/111320、lat = z/111320——与 graph-debugger 一致）。
/// Web Mercator 瓦片网格：lng/lat → tile (z, x, y)。
/// </summary>
public static class MvtEncoder
{
    public const int Extent = 4096;

    /// <summary>游戏坐标 → 经纬度（度）。</summary>
    public static (double Lng, double Lat) ToLngLat(double x, double z) => (x / 111320.0, z / 111320.0);

    /// <summary>经纬度 → Web Mercator 瓦片坐标（像素，0..2^z×extent）。</summary>
    public static (double Px, double Py) Project(double lng, double lat, int z)
    {
        double n = Math.Pow(2, z);
        double px = (lng + 180.0) / 360.0 * n * Extent;
        double latRad = lat * Math.PI / 180.0;
        double py = (1.0 - Math.Log(Math.Tan(latRad) + 1.0 / Math.Cos(latRad)) / Math.PI) / 2.0 * n * Extent;
        return (px, py);
    }

    public static (int X, int Y) TileXY(double lng, double lat, int z)
    {
        double n = Math.Pow(2, z);
        int x = (int)Math.Floor((lng + 180.0) / 360.0 * n);
        double latRad = lat * Math.PI / 180.0;
        int y = (int)Math.Floor((1.0 - Math.Log(Math.Tan(latRad) + 1.0 / Math.Cos(latRad)) / Math.PI) / 2.0 * n);
        if (x < 0) x = 0;
        if (x >= (1 << z)) x = (1 << z) - 1;
        if (y < 0) y = 0;
        if (y >= (1 << z)) y = (1 << z) - 1;
        return (x, y);
    }

    /// <summary>瓦片内的几何要素。</summary>
    public sealed class Feature
    {
        public required int Type { get; init; }        // 1=Point 2=LineString 3=Polygon
        public required double[][] Points { get; init; }   // 瓦片像素坐标（可多段）
        public Dictionary<string, object>? Tags { get; init; }
    }

    /// <summary>编码一个瓦片（多图层）。</summary>
    public static byte[] EncodeTile(Dictionary<string, List<Feature>> layers)
    {
        var tile2 = new List<byte>();
        foreach (var (name, features) in layers)
        {
            var layerBytes = EncodeLayer(name, features);
            tile2.AddRange(EncodeTag(3, 2));
            tile2.AddRange(EncodeVarint((ulong)layerBytes.Length));
            tile2.AddRange(layerBytes);
        }
        return tile2.ToArray();
    }

    private static byte[] EncodeLayer(string name, List<Feature> features)
    {
        var field = new List<byte>();
        // version = 2（field 15, varint）
        field.AddRange(EncodeTag(15, 0));
        field.AddRange(EncodeVarint(2));
        // name（field 1, string）
        var nameBytes = System.Text.Encoding.UTF8.GetBytes(name);
        field.AddRange(EncodeTag(1, 2));
        field.AddRange(EncodeVarint((ulong)nameBytes.Length));
        field.AddRange(nameBytes);
        // keys/values 表
        var keyTable = new List<string>();
        var valueTable = new List<object>();
        foreach (var f in features)
        {
            if (f.Tags == null) continue;
            foreach (var (k, v) in f.Tags)
            {
                if (!keyTable.Contains(k)) keyTable.Add(k);
                if (!valueTable.Contains(v)) valueTable.Add(v);
            }
        }
        foreach (var k in keyTable)
        {
            var kb = System.Text.Encoding.UTF8.GetBytes(k);
            field.AddRange(EncodeTag(3, 2));
            field.AddRange(EncodeVarint((ulong)kb.Length));
            field.AddRange(kb);
        }
        foreach (var v in valueTable)
        {
            var vb = EncodeValue(v);
            field.AddRange(EncodeTag(4, 2));
            field.AddRange(EncodeVarint((ulong)vb.Length));
            field.AddRange(vb);
        }
        // extent = 4096（field 5, varint）
        field.AddRange(EncodeTag(5, 0));
        field.AddRange(EncodeVarint(Extent));
        // features
        foreach (var f in features)
        {
            var fb = EncodeFeature(f, keyTable, valueTable);
            field.AddRange(EncodeTag(2, 2));
            field.AddRange(EncodeVarint((ulong)fb.Length));
            field.AddRange(fb);
        }
        return field.ToArray();
    }

    private static byte[] EncodeFeature(Feature f, List<string> keys, List<object> values)
    {
        var b = new List<byte>();
        // type（field 3, varint）
        b.AddRange(EncodeTag(3, 0));
        b.AddRange(EncodeVarint((ulong)f.Type));
        // tags（field 2, packed）
        if (f.Tags != null && f.Tags.Count > 0)
        {
            var tags = new List<byte>();
            foreach (var (k, v) in f.Tags)
            {
                int ki = keys.IndexOf(k);
                int vi = values.IndexOf(v);
                tags.AddRange(EncodeVarint((ulong)ki));
                tags.AddRange(EncodeVarint((ulong)vi));
            }
            b.AddRange(EncodeTag(2, 2));
            b.AddRange(EncodeVarint((ulong)tags.Count));
            b.AddRange(tags);
        }
        // geometry（field 4, packed）
        var geom = EncodeGeometry(f);
        b.AddRange(EncodeTag(4, 2));
        b.AddRange(EncodeVarint((ulong)geom.Count));
        b.AddRange(geom);
        return b.ToArray();
    }

    internal static List<byte> EncodeGeometry(Feature f)
    {
        var g = new List<byte>();
        int cx = 0, cy = 0;
        for (int seg = 0; seg < f.Points.Length; seg++)
        {
            var pts = f.Points[seg];
            if (pts.Length == 0) continue;
            // MoveTo
            g.AddRange(EncodeVarint((ulong)(1 | (1 << 3))));
            g.AddRange(EncodeVarint(Zigzag((long)pts[0] - cx)));
            g.AddRange(EncodeVarint(Zigzag((long)pts[1] - cy)));
            cx = (int)pts[0];
            cy = (int)pts[1];
            if (f.Type == 1) continue;   // Point：只有 MoveTo
            // LineTo（pts 扁平：x,y,x,y…）
            int pointCount = pts.Length / 2;
            if (pointCount > 1)
            {
                g.AddRange(EncodeVarint((ulong)(2 | ((pointCount - 1) << 3))));
                for (int i = 1; i < pointCount; i++)
                {
                    g.AddRange(EncodeVarint(Zigzag((long)pts[i * 2] - cx)));
                    g.AddRange(EncodeVarint(Zigzag((long)pts[i * 2 + 1] - cy)));
                    cx = (int)pts[i * 2];
                    cy = (int)pts[i * 2 + 1];
                }
            }
        }
        return g;
    }

    /// <summary>
    /// MVT <c>Value</c> 编码。字段号取自 vector_tile.proto：
    /// 1=string_value(2) / 2=float_value(5) / 3=double_value(1) / 4=int_value(0) /
    /// 7=bool_value(0)。括号内为 wire type。
    ///
    /// 2026-09 修复：整数原先写为 field 2（wire 0），而 field 2 是 float_value，
    /// wire type 必须为 5——该值声明与线型不符，MapLibre 解析时静默丢弃整个要素，
    /// 导致带 speed_limit 的 road 与带 movements 的 junction 在浏览器中完全不渲染。
    /// 整数应使用 field 4（int_value）。
    /// 同时移除「未知类型产出一个零字节 Value」的静默行为，改为显式失败。
    /// </summary>
    internal static byte[] EncodeValue(object v)
    {
        var b = new List<byte>();
        switch (v)
        {
            case string s:
                var sb = System.Text.Encoding.UTF8.GetBytes(s);
                b.AddRange(EncodeTag(1, 2));
                b.AddRange(EncodeVarint((ulong)sb.Length));
                b.AddRange(sb);
                break;
            case int i:
                b.AddRange(EncodeTag(4, 0)); // int_value
                b.AddRange(EncodeVarint(unchecked((ulong)(long)i)));
                break;
            case long l:
                b.AddRange(EncodeTag(4, 0)); // int_value
                b.AddRange(EncodeVarint(unchecked((ulong)l)));
                break;
            case double d:
                b.AddRange(EncodeTag(3, 1)); // double_value
                b.AddRange(BitConverter.GetBytes(d));
                break;
            default:
                throw new NotSupportedException(
                    $"MVT Value 不支持的类型 {v.GetType().Name}（只支持 string/int/long/double）；" +
                    "拒绝静默写入空 Value。");
        }
        return b.ToArray();
    }

    // —— protobuf 基础 ——
    private static List<byte> EncodeTag(int field, int wireType) => EncodeVarint((ulong)((field << 3) | wireType));

    private static List<byte> EncodeVarint(ulong v)
    {
        var b = new List<byte>();
        while (v >= 0x80)
        {
            b.Add((byte)(v | 0x80));
            v >>= 7;
        }
        b.Add((byte)v);
        return b;
    }

    private static ulong Zigzag(long n) => (ulong)((n << 1) ^ (n >> 63));
}
