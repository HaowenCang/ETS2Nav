using ScsResource;

namespace ScsPrefab;

/// <summary>
/// Prefab 描述文件（.ppd）解析器——独立实现，对照 TruckLib.Models Ppd 结构（oracle，ADR-005）。
/// 支持 v0x19（1.60 实测版本）；Sign/SpawnPoint/MapPoint/TriggerPoint 等非导航段仅跳过字节。
/// </summary>
public static class PpdReader
{
    public static PrefabDescriptor Read(Stream s, string sourcePath)
    {
        using var r = new BinaryReader(s, System.Text.Encoding.UTF8, leaveOpen: true);
        uint version = r.ReadUInt32();
        if (version != 0x19)
            throw new NotSupportedException($"PPD 版本 0x{version:x} 不支持（{sourcePath}，需要 0x19）");

        uint nodeCount = r.ReadUInt32();
        uint navCurveCount = r.ReadUInt32();
        uint signCount = r.ReadUInt32();
        uint semaphoreCount = r.ReadUInt32();
        uint spawnPointCount = r.ReadUInt32();
        uint terrainPointCount = r.ReadUInt32();
        uint terrainPointVariantCount = r.ReadUInt32();
        uint mapPointCount = r.ReadUInt32();
        uint triggerPointCount = r.ReadUInt32();
        uint intersectionCount = r.ReadUInt32();
        uint navNodeCount = r.ReadUInt32();
        for (int i = 0; i < 12; i++) r.ReadUInt32();   // offsets（可忽略，顺序布局）

        var pd = new PrefabDescriptor { Version = version, SourcePath = sourcePath };

        for (int i = 0; i < nodeCount; i++) pd.ControlNodes.Add(ReadControlNode(r));
        for (int i = 0; i < navCurveCount; i++) pd.NavCurves.Add(ReadNavCurve(r));
        for (int i = 0; i < signCount; i++) SkipSign(r);
        for (int i = 0; i < semaphoreCount; i++) pd.Semaphores.Add(ReadSemaphore(r));
        for (int i = 0; i < spawnPointCount; i++) SkipSpawnPoint(r);
        for (int i = 0; i < terrainPointCount; i++) { r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); }
        for (int i = 0; i < terrainPointCount; i++) { r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); }
        for (int i = 0; i < terrainPointVariantCount; i++) { r.ReadUInt32(); r.ReadUInt32(); }
        for (int i = 0; i < mapPointCount; i++) SkipMapPoint(r);
        for (int i = 0; i < triggerPointCount; i++) SkipTriggerPoint(r);
        for (int i = 0; i < intersectionCount; i++) pd.Intersections.Add(ReadIntersection(r));
        for (int i = 0; i < navNodeCount; i++) pd.NavNodes.Add(ReadNavNode(r));

        return pd;
    }

    private static ControlNodeData ReadControlNode(BinaryReader r)
    {
        r.ReadUInt32(); r.ReadUInt32(); r.ReadUInt32(); r.ReadUInt32();   // terrain 索引/计数
        var c = new ControlNodeData
        {
            X = r.ReadSingle(), Y = r.ReadSingle(), Z = r.ReadSingle(),
            DirX = r.ReadSingle(), DirY = r.ReadSingle(), DirZ = r.ReadSingle(),
        };
        for (int i = 0; i < 8; i++) c.InputLines[i] = r.ReadInt32();
        for (int i = 0; i < 8; i++) c.OutputLines[i] = r.ReadInt32();
        return c;
    }

    private static NavCurveData ReadNavCurve(BinaryReader r)
    {
        string name = ReadToken(r);
        uint flags = r.ReadUInt32();
        byte endNode = r.ReadByte();
        byte endLane = r.ReadByte();
        byte startNode = r.ReadByte();
        byte startLane = r.ReadByte();
        float sx = r.ReadSingle(), sy = r.ReadSingle(), sz = r.ReadSingle();
        float ex = r.ReadSingle(), ey = r.ReadSingle(), ez = r.ReadSingle();
        r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); r.ReadSingle();   // start quat
        r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); r.ReadSingle();   // end quat
        float length = r.ReadSingle();
        var c = new NavCurveData
        {
            Name = name, Flags = flags,
            EndNode = endNode, EndLane = endLane, StartNode = startNode, StartLane = startLane,
            StartX = sx, StartY = sy, StartZ = sz,
            EndX = ex, EndY = ey, EndZ = ez,
            Length = length, SemaphoreId = -1, TrafficRule = "", NavNodeIndex = 0xFFFFFFFF,
            NextCount = 0, PreviousCount = 0,
        };
        for (int i = 0; i < 4; i++) c.NextLines[i] = r.ReadInt32();
        for (int i = 0; i < 4; i++) c.PreviousLines[i] = r.ReadInt32();
        c.NextCount = (int)r.ReadUInt32();
        c.PreviousCount = (int)r.ReadUInt32();
        c.SemaphoreId = r.ReadInt32();
        c.TrafficRule = ReadToken(r);
        c.NavNodeIndex = r.ReadUInt32();
        return c;
    }

    private static void SkipSign(BinaryReader r)
    {
        ReadToken(r);
        r.ReadSingle(); r.ReadSingle(); r.ReadSingle();
        r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); r.ReadSingle();
        ReadToken(r);
        ReadToken(r);
    }

    private static SemaphoreData ReadSemaphore(BinaryReader r)
    {
        float x = r.ReadSingle(), y = r.ReadSingle(), z = r.ReadSingle();
        float rx = r.ReadSingle(), ry = r.ReadSingle(), rz = r.ReadSingle(), rw = r.ReadSingle();
        uint type = r.ReadUInt32();
        uint id = r.ReadUInt32();
        float ix = r.ReadSingle(), iy = r.ReadSingle(), iz = r.ReadSingle(), iw = r.ReadSingle();
        float delay = r.ReadSingle();
        ulong raw = r.ReadUInt64();
        string profile = DecodeToken(raw);
        r.ReadUInt32();               // unknown1
        for (int i = 0; i < 4; i++) r.ReadUInt32();   // unknown2
        return new SemaphoreData
        {
            Profile = profile, TokenRaw = raw, Type = type, X = x, Y = y, Z = z,
            Rx = rx, Ry = ry, Rz = rz, Rw = rw,
            SemaphoreId = id, Ix = ix, Iy = iy, Iz = iz, Iw = iw, CycleDelay = delay,
        };
    }

    private static void SkipSpawnPoint(BinaryReader r)
    {
        r.ReadSingle(); r.ReadSingle(); r.ReadSingle();
        r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); r.ReadSingle();
        r.ReadUInt32();   // type
        r.ReadUInt32();   // flags
    }

    private static void SkipMapPoint(BinaryReader r)
    {
        r.ReadUInt32();   // visFlags
        r.ReadUInt32();   // navFlags
        r.ReadSingle(); r.ReadSingle(); r.ReadSingle();
        for (int i = 0; i < 6; i++) r.ReadInt32();
        r.ReadUInt32();   // used
    }

    private static void SkipTriggerPoint(BinaryReader r)
    {
        r.ReadUInt32();
        ReadToken(r);
        r.ReadSingle(); r.ReadSingle(); r.ReadSingle();
        r.ReadUInt32();
        r.ReadSingle(); r.ReadSingle(); r.ReadSingle();
        r.ReadInt32(); r.ReadInt32();
    }

    private static IntersectionData ReadIntersection(BinaryReader r) => new()
    {
        CurveId = r.ReadUInt32(),
        Position = r.ReadSingle(),
        Radius = r.ReadSingle(),
        Flags = r.ReadUInt32(),
    };

    private static NavNodeData ReadNavNode(BinaryReader r)
    {
        byte type = r.ReadByte();
        ushort index = r.ReadUInt16();
        byte used = r.ReadByte();
        var n = new NavNodeData { Type = type, Index = index };
        for (int i = 0; i < 8; i++)
        {
            ushort target = r.ReadUInt16();
            float len = r.ReadSingle();
            byte usedCurves = r.ReadByte();
            var conn = new NavNodeConnectionData { TargetNodeIndex = target, Length = len };
            for (int k = 0; k < 8; k++)
            {
                ushort ci = r.ReadUInt16();
                if (k < usedCurves) conn.CurveIndices.Add(ci);
            }
            if (i < used) n.Connections.Add(conn);
        }
        return n;
    }

    private static string ReadToken(BinaryReader r)
    {
        ulong v = r.ReadUInt64();
        return DecodeToken(v);
    }

    /// <summary>SCS token 字符集（38 字符：\0 + 0-9 + a-z + _）。</summary>
    private static readonly char[] Charset =
    {
        '\0', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
        'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w',
        'x', 'y', 'z', '_'
    };

    /// <summary>8 字节 base-38 token 解码（同 ScsSector.ScsBinary）。
    /// 超出 base-38 范围的值（≥38^12，SCS 哈希 token）返回 &amp;0x… 标记而非崩溃。</summary>
    private static string DecodeToken(ulong value)
    {
        if (value == 0) return "";
        int length = 1;
        while (Pow38(length) - 1 < value && length < 12) length++;
        if (Pow38(length) - 1 < value)
            return "&0x" + value.ToString("x");   // 哈希 token（特殊字符/超长名）
        var chars = new char[length];
        for (int i = length; i > 0; i--)
        {
            ulong pow = Pow38(i - 1);
            long digit = (long)(value / pow);
            if (digit < 0 || digit >= Charset.Length) return "&0x" + value.ToString("x");
            chars[length - i] = Charset[(int)digit];
            value %= pow;
        }
        return new string(chars);
    }

    private static ulong Pow38(int n)
    {
        ulong r = 1;
        for (int i = 0; i < n; i++) r *= 38;
        return r;
    }
}
