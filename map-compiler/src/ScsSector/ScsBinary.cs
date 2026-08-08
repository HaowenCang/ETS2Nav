// SCS 地图二进制格式基础读取器（独立实现）
// Token：8 字节 u64，base-38 编码（字符集 \0 0-9 a-z _），低位=首字符
// 固定点：i32 × 1/256（坐标）、1/10 等
// MIT License — ETS2Nav 项目

using System.Text;

namespace ScsSector;

public static class ScsBinary
{
    private static readonly char[] Charset =
    [
        '\0', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
        'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w',
        'x', 'y', 'z', '_'
    ];

    /// <summary>解码 8 字节 token（u64 LE，base-38，低位=首字符）。</summary>
    public static string DecodeToken(ulong value)
    {
        if (value == 0) return "";
        int length = 1;
        while (Pow38(length) - 1 < value) length++;
        var chars = new char[length];
        for (int i = length; i > 0; i--)
        {
            ulong pow = Pow38(i - 1);
            int idx = (int)(value / pow);
            chars[i - 1] = Charset[idx];
            value -= (ulong)idx * pow;
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

public static class BinaryReaderExtensions
{
    public static string ReadToken(this BinaryReader r) => ScsBinary.DecodeToken(r.ReadUInt64());

    /// <summary>i32 固定点（1/256），返回浮点值。</summary>
    public static double ReadFixed256(this BinaryReader r) => r.ReadInt32() / 256.0;

    public static (double X, double Y, double Z) ReadFixed3(this BinaryReader r) =>
        (r.ReadFixed256(), r.ReadFixed256(), r.ReadFixed256());

    public static (float X, float Y, float Z, float W) ReadQuaternion(this BinaryReader r) =>
        (r.ReadSingle(), r.ReadSingle(), r.ReadSingle(), r.ReadSingle());

    /// <summary>u32 count + count × u64 引用数组。</summary>
    public static ulong[] ReadUidArray(this BinaryReader r)
    {
        uint count = r.ReadUInt32();
        var arr = new ulong[count];
        for (int i = 0; i < count; i++) arr[i] = r.ReadUInt64();
        return arr;
    }

    /// <summary>u32 count + count × token 数组。</summary>
    public static string[] ReadTokenArray(this BinaryReader r)
    {
        uint count = r.ReadUInt32();
        var arr = new string[count];
        for (int i = 0; i < count; i++) arr[i] = r.ReadToken();
        return arr;
    }
}
