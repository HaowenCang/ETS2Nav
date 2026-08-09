// HashFS（.scs 容器）读取器 v1/v2 独立实现。
// 格式布局：社区逆向结论（见 docs/format-notes/hashfs.md），本实现独立编写。
// v2 于游戏 1.50 引入：entry/metadata 表 zlib 压缩、二进制目录列表、打包纹理（GDeflate，暂不支持提取）。
// GPL-3.0 — ETS2Nav 项目

using System.IO.Compression;
using System.Text;

namespace ScsHashFs;

public enum EntryType { Directory, File, PackedImage }

public sealed record HashFsEntry(
    ulong Hash,
    long Offset,
    int Size,
    int CompressedSize,
    bool IsCompressed,
    EntryType Type);

public sealed class HashFsReader : IDisposable
{
    private const uint Magic = 0x23534353; // "SCS#"

    private readonly Stream _s;
    private readonly ushort _salt;
    private readonly Dictionary<ulong, HashFsEntry> _entries;

    private HashFsReader(Stream s, ushort salt, bool isV2, Dictionary<ulong, HashFsEntry> entries)
    {
        _s = s;
        _salt = salt;
        _isV2 = isV2;
        _entries = entries;
    }

    public ushort Salt => _salt;
    public int EntryCount => _entries.Count;

    private readonly bool _isV2;

    public static HashFsReader Open(string path)
    {
        var s = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
        try
        {
            var reader = new BinaryReader(s, Encoding.UTF8, leaveOpen: true);
            Span<byte> hdr = stackalloc byte[16];
            if (s.Read(hdr) < 16) throw new HashFsException("不是 HashFS 归档（文件过短）");
            if (BitConverter.ToUInt32(hdr) != Magic) throw new HashFsException("不是 HashFS 归档（magic 不符）");
            int version = hdr[4] | (hdr[5] << 8);
            return version switch
            {
                1 => OpenV1(s, reader),
                2 => OpenV2(s, reader),
                _ => throw new HashFsException($"不支持的 HashFS 版本：{version}")
            };
        }
        catch
        {
            s.Dispose();
            throw;
        }
    }

    private static HashFsReader OpenV1(Stream s, BinaryReader r)
    {
        // header: magic(4) version(u16) salt(u16) "CITY"(4) num_entries(u32) start(u64)
        r.BaseStream.Position = 6;
        ushort salt = r.ReadUInt16();
        r.BaseStream.Position = 12;
        int numEntries = r.ReadInt32();
        long start = r.ReadInt64();
        s.Position = start;
        var table = r.ReadBytes(numEntries * 32);
        var entries = new Dictionary<ulong, HashFsEntry>(numEntries);
        for (int i = 0; i < numEntries; i++)
        {
            int p = i * 32;
            ulong hash = BitConverter.ToUInt64(table, p);
            long offset = (long)BitConverter.ToUInt64(table, p + 8);
            uint flags = BitConverter.ToUInt32(table, p + 16);
            int size = BitConverter.ToInt32(table, p + 24);
            int csize = BitConverter.ToInt32(table, p + 28);
            bool isDir = (flags & 1) != 0;
            bool compressed = (flags & 2) != 0;
            entries[hash] = new HashFsEntry(hash, offset, size, csize, compressed,
                isDir ? EntryType.Directory : EntryType.File);
        }
        return new HashFsReader(s, salt, isV2: false, entries);
    }

    private static HashFsReader OpenV2(Stream s, BinaryReader r)
    {
        // header: magic(4) version(u16) salt(u16) "CITY"(4) entry_count(u32)
        //         entry_table_len(u32) ?(u32) metadata_table_len(u32)
        //         entry_table_start(u64) metadata_table_start(u64)
        r.BaseStream.Position = 6;
        ushort salt = r.ReadUInt16();
        r.BaseStream.Position = 16;
        int entryTableLen = r.ReadInt32();
        r.BaseStream.Position = 24;
        int metadataTableLen = r.ReadInt32();
        long entryTableStart = r.ReadInt64();
        long metadataTableStart = r.ReadInt64();

        s.Position = entryTableStart;
        var entryTable = ZlibDecompress(r.ReadBytes(entryTableLen));
        s.Position = metadataTableStart;
        var meta = ZlibDecompress(r.ReadBytes(metadataTableLen));

        var entries = new Dictionary<ulong, HashFsEntry>(entryTable.Length / 16);
        int count = entryTable.Length / 16;
        for (int i = 0; i < count; i++)
        {
            int p = i * 16;
            ulong hash = BitConverter.ToUInt64(entryTable, p);
            int metaIndex = BitConverter.ToInt32(entryTable, p + 8);
            ushort metaCount = BitConverter.ToUInt16(entryTable, p + 12);
            if (metaIndex < 0) continue;
            var entry = DecodeV2Metadata(meta, metaIndex, metaCount);
            if (entry != null) entries[hash] = entry;
        }
        return new HashFsReader(s, salt, isV2: true, entries);
    }

    // v2 metadata：块头 4 字节（3 索引字节 + 1 类型字节），主体在 metaIndex*4 + metaCount*4
    private static HashFsEntry? DecodeV2Metadata(byte[] meta, int metaIndex, int metaCount)
    {
        int pos = metaIndex * 4;
        if (pos + 4 > meta.Length) return null;
        byte chunkType = meta[pos + 3];
        int body = pos + metaCount * 4;
        if (body + 16 > meta.Length) return null;

        bool isImage = chunkType == 1;
        if (!isImage && chunkType != 128 && chunkType != 129) return null; // 未知块类型

        if (isImage) body += 12; // packed tobj/dds 元数据在前
        ulong csize = (ulong)meta[body] | ((ulong)meta[body + 1] << 8) |
                      ((ulong)meta[body + 2] << 16) | (((ulong)meta[body + 3] & 0x0F) << 24);
        bool compressed = (meta[body + 3] & 0x10) != 0;
        ulong size = (ulong)meta[body + 4] | ((ulong)meta[body + 5] << 8) |
                     ((ulong)meta[body + 6] << 16) | (((ulong)meta[body + 7] & 0x0F) << 24);
        uint offsetBlock = BitConverter.ToUInt32(meta, body + 12);
        long offset = (long)offsetBlock * 16;

        var type = isImage ? EntryType.PackedImage
            : chunkType == 129 ? EntryType.Directory : EntryType.File;
        return new HashFsEntry(0, offset, (int)size, (int)csize, compressed, type);
    }

    private static byte[] ZlibDecompress(byte[] data)
    {
        using var input = new MemoryStream(data);
        using var z = new ZLibStream(input, CompressionMode.Decompress);
        using var output = new MemoryStream();
        z.CopyTo(output);
        return output.ToArray();
    }

    /// <summary>按路径查条目（自动加盐哈希）。</summary>
    public HashFsEntry? TryGetEntry(string path)
    {
        var h = CityHash.HashPath(path, _salt);
        return _entries.TryGetValue(h, out var e) ? e : null;
    }

    /// <summary>提取条目原始数据（目录条目返回其二进制目录列表）。</summary>
    public byte[] Extract(string path)
    {
        var entry = TryGetEntry(path) ?? throw new HashFsException($"归档中不存在：{path}");
        return Extract(entry);
    }

    public byte[] Extract(HashFsEntry entry)
    {
        if (entry.Type == EntryType.PackedImage)
            throw new HashFsException("打包纹理条目（GDeflate）暂不支持提取");
        _s.Position = entry.Offset;
        byte[] raw = new byte[entry.CompressedSize];
        _s.ReadExactly(raw);
        if (!entry.IsCompressed) return raw;
        return ZlibDecompress(raw);
    }

    /// <summary>提取为 UTF-8 文本（SII/文本资源）。</summary>
    public string ExtractText(string path) => Encoding.UTF8.GetString(Extract(path));

    /// <summary>枚举目录内容（v1 文本列表 / v2 二进制列表）。返回（子目录，文件）。</summary>
    public (List<string> Subdirs, List<string> Files) ListDirectory(string path)
    {
        var entry = TryGetEntry(path) ?? throw new HashFsException($"归档中不存在目录：{path}");
        if (entry.Type != EntryType.Directory)
            throw new HashFsException($"{path} 不是目录条目");
        var blob = Extract(entry);
        return ParseDirectoryListing(blob, _isV2);
    }

    private static (List<string>, List<string>) ParseDirectoryListing(byte[] blob, bool isV2)
    {
        var subdirs = new List<string>();
        var files = new List<string>();
        if (!isV2)
        {
            // v1：文本，一行一项，子目录带 '*' 前缀（如 "*subdir"）
            var text = Encoding.UTF8.GetString(blob);
            foreach (var line in text.Split('\n', StringSplitOptions.RemoveEmptyEntries))
            {
                var t = line.Trim();
                if (t.Length == 0) continue;
                if (t.StartsWith('*')) subdirs.Add(t[1..]);
                else files.Add(t);
            }
            return (subdirs, files);
        }

        // v2：u32 count + count 字节的 name-length + names（'/' 前缀=目录）
        if (blob.Length < 4) return (subdirs, files);
        int count = BitConverter.ToInt32(blob, 0);
        int pos = 4 + count;   // 跳过 lengths 数组
        if (pos > blob.Length) return (subdirs, files);
        for (int i = 0; i < count; i++)
        {
            int len = blob[4 + i];   // lengths 数组第 i 项（names 区从 pos 开始）
            if (pos + len > blob.Length) break;
            var name = Encoding.UTF8.GetString(blob, pos, len);
            pos += len;
            if (name.StartsWith('/')) subdirs.Add(name[1..]);
            else files.Add(name);
        }
        return (subdirs, files);
    }

    /// <summary>递归枚举归档内全部文件路径（依赖目录列表存在；缺失时仅返回可枚举部分）。</summary>
    public IEnumerable<string> EnumerateFiles(string root = "/")
    {
        var stack = new Stack<(string Dir, bool Listed)>();
        stack.Push((root, false));
        var seen = new HashSet<string>();
        while (stack.Count > 0)
        {
            var (dir, listed) = stack.Pop();
            if (!listed)
            {
                var entry = TryGetEntry(dir);
                if (entry == null || entry.Type != EntryType.Directory) continue;
                if (!seen.Add(dir)) continue;
            }
            var (subdirs, files) = ListDirectory(dir);
            foreach (var f in files) yield return Combine(dir, f);
            foreach (var d in subdirs) stack.Push((Combine(dir, d), false));
        }
    }

    private static string Combine(string dir, string name) =>
        dir == "/" || dir.EndsWith('/') ? dir + name : dir + "/" + name;

    public void Dispose() => _s.Dispose();
}

public sealed class HashFsException : Exception
{
    public HashFsException(string message) : base(message) { }
}
