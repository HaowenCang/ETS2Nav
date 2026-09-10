// ScsResource：SCS zipfs（ZIP 容器）条目名探测（P6 §9 覆盖缺口修复，2026-08-12）。
//
// 动机：ETS2 的 mod 容器有两种——HashFS（.scs，magic 头）与 zipfs（标准 ZIP，可为 .scs 或 .zip）。
// 本项目 HashFsReader 只读前者；而**最关键的地图 mod 恰为 zipfs**（实测本机 ProMods：
// game.log 记 `[zipfs] promods-eu-map-v281.scs: Created, 40594 entries`），故仅靠 HashFS
// 探测会把「含 /map 的地图 mod」误判为无关，使 mod 指纹失去地图相关性分层能力。
//
// 本文件只做**条目名枚举**（不列目录、不解压、不读内容）：读 ZIP64/EOCD 定位中央目录，
// 顺序解析中央目录记录取文件名。用途单一——判定 archive 是否含 /map 前缀条目。
//
// GPL-3.0 — ETS2Nav 项目

using System.Buffers.Binary;
using System.Text;

namespace ScsResource;

/// <summary>ZIP（zipfs）条目名枚举——仅中央目录，不解压。</summary>
public static class ZipfsProbe
{
    private const uint EocdSig = 0x06054b50;
    private const uint Zip64LocatorSig = 0x07064b50;
    private const uint Zip64EocdSig = 0x06064b50;
    private const uint CentralSig = 0x02014b50;
    private const uint Zip64ExtraId = 0x0001;

    /// <summary>是否为 ZIP 容器（读首 4 字节判 PK\x03\x04）。</summary>
    public static bool LooksLikeZip(string path)
    {
        try
        {
            using var fs = File.OpenRead(path);
            Span<byte> sig = stackalloc byte[4];
            if (fs.Read(sig) < 4) return false;
            return sig[0] == 'P' && sig[1] == 'K' && sig[2] == 3 && sig[3] == 4;
        }
        catch
        {
            return false;
        }
    }

    /// <summary>枚举中央目录中的条目名。损坏/非 ZIP 时抛异常（调用方决定降级策略）。</summary>
    public static IEnumerable<string> EnumerateEntryNames(string path)
    {
        using var fs = File.OpenRead(path);
        long fileLen = fs.Length;
        if (fileLen < 22) throw new InvalidDataException("文件过小，非 ZIP");

        // —— 1) 从尾部回扫 EOCD（注释最长 65535 字节）——
        int scanLen = (int)Math.Min(fileLen, 22 + 65535);
        var tail = new byte[scanLen];
        fs.Seek(fileLen - scanLen, SeekOrigin.Begin);
        ReadExactly(fs, tail);
        long eocdPos = -1;
        for (int i = scanLen - 22; i >= 0; i--)
        {
            if (BinaryPrimitives.ReadUInt32LittleEndian(tail.AsSpan(i)) == EocdSig)
            {
                eocdPos = fileLen - scanLen + i;
                break;
            }
        }
        if (eocdPos < 0) throw new InvalidDataException("未找到 EOCD（非 ZIP）");

        var eocd = new byte[22];
        fs.Seek(eocdPos, SeekOrigin.Begin);
        ReadExactly(fs, eocd);
        long entryCount = BinaryPrimitives.ReadUInt16LittleEndian(eocd.AsSpan(10));
        long cdOffset = BinaryPrimitives.ReadUInt32LittleEndian(eocd.AsSpan(16));

        // —— 2) ZIP64：EOCD 字段饱和时改读 ZIP64 EOCD ——
        bool zip64 = entryCount == 0xFFFF || cdOffset == 0xFFFFFFFF;
        long locatorPos = eocdPos - 20;
        if (zip64 || locatorPos >= 0)
        {
            var loc = new byte[20];
            fs.Seek(locatorPos, SeekOrigin.Begin);
            ReadExactly(fs, loc);
            if (BinaryPrimitives.ReadUInt32LittleEndian(loc) == Zip64LocatorSig)
            {
                long z64Pos = (long)BinaryPrimitives.ReadUInt64LittleEndian(loc.AsSpan(8));
                var z64 = new byte[56];
                fs.Seek(z64Pos, SeekOrigin.Begin);
                ReadExactly(fs, z64);
                if (BinaryPrimitives.ReadUInt32LittleEndian(z64) != Zip64EocdSig)
                    throw new InvalidDataException("ZIP64 EOCD 签名不符");
                entryCount = (long)BinaryPrimitives.ReadUInt64LittleEndian(z64.AsSpan(32));
                cdOffset = (long)BinaryPrimitives.ReadUInt64LittleEndian(z64.AsSpan(48));
            }
            else if (zip64)
            {
                throw new InvalidDataException("ZIP64 标记存在但无定位器");
            }
        }

        // —— 3) 顺序解析中央目录记录 ——
        fs.Seek(cdOffset, SeekOrigin.Begin);
        var hdr = new byte[46];
        for (long n = 0; n < entryCount; n++)
        {
            ReadExactly(fs, hdr);
            if (BinaryPrimitives.ReadUInt32LittleEndian(hdr) != CentralSig) yield break;   // 结构异常，安全停止
            int nameLen = BinaryPrimitives.ReadUInt16LittleEndian(hdr.AsSpan(28));
            int extraLen = BinaryPrimitives.ReadUInt16LittleEndian(hdr.AsSpan(30));
            int commentLen = BinaryPrimitives.ReadUInt16LittleEndian(hdr.AsSpan(32));
            if (nameLen <= 0 || nameLen > 4096) yield break;
            var nameBuf = new byte[nameLen];
            ReadExactly(fs, nameBuf);
            yield return Encoding.UTF8.GetString(nameBuf);
            long skip = (long)extraLen + commentLen;
            if (skip > 0) fs.Seek(skip, SeekOrigin.Current);
        }
    }

    /// <summary>是否含以 <paramref name="prefix"/> 开头的条目（如 "/map"）。命中即返回，不枚举全表。</summary>
    public static bool ContainsPrefix(string path, string prefix)
    {
        // SCS zipfs 条目名可能带或不带前导 '/'，两种都接受
        var p1 = prefix.StartsWith('/') ? prefix : "/" + prefix;
        var p2 = prefix.TrimStart('/');
        foreach (var name in EnumerateEntryNames(path))
        {
            if (name.StartsWith(p1, StringComparison.OrdinalIgnoreCase)
                || name.StartsWith(p2, StringComparison.OrdinalIgnoreCase))
                return true;
        }
        return false;
    }

    private static void ReadExactly(Stream s, byte[] buf)
    {
        int off = 0;
        while (off < buf.Length)
        {
            int r = s.Read(buf, off, buf.Length - off);
            if (r <= 0) throw new EndOfStreamException("读取越界（archive 结构异常）");
            off += r;
        }
    }
}
