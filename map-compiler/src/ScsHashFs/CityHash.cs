// CityHash64 独立实现（SCS HashFS 使用的变体，与 cityhash-c 移植版逐操作一致）。
// 算法公开规范；本实现以 64 位无符号运算 + unchecked 溢出（等价于 Python 参考的 & MASK）。
// 用途：HashFS 路径哈希 CityHash64(utf8(salt + path_without_leading_slash))。
// GPL-3.0 — ETS2Nav 项目

namespace ScsHashFs;

public static class CityHash
{
    private const ulong K0 = 0xC3A5C85C97CB3127UL;
    private const ulong K1 = 0xB492B66FBE98F273UL;
    private const ulong K2 = 0x9AE16A3B2F90404FUL;
    private const ulong K3 = 0xC949D7C7509E6557UL;
    private const ulong KMul = 0x9DDFEA08EB382D69UL;

    public static ulong CityHash64(ReadOnlySpan<byte> s)
    {
        int len = s.Length;
        if (len <= 16) return HashLen0To16(s, len);
        if (len <= 32) return HashLen17To32(s, len);
        if (len <= 64) return HashLen33To64(s, len);
        return HashLen65Plus(s, len);
    }

    /// <summary>HashFS 路径哈希：去前导 /，非零 salt 前置十进制串。</summary>
    public static ulong HashPath(string path, ushort salt = 0)
    {
        var p = path.StartsWith('/') ? path[1..] : path;
        var full = salt != 0 ? salt.ToString() + p : p;
        return CityHash64(System.Text.Encoding.UTF8.GetBytes(full));
    }

    // ---- 原语 ----

    private static ulong Rotate(ulong v, int shift) => (v >> shift) | (v << (64 - shift));
    private static ulong RotateByAtLeast1(ulong v, int shift) => (v >> shift) | (v << (64 - shift));
    private static ulong ShiftMix(ulong v) => v ^ (v >> 47);

    private static ulong Fetch32(ReadOnlySpan<byte> s, int i) =>
        (ulong)s[i] | ((ulong)s[i + 1] << 8) | ((ulong)s[i + 2] << 16) | ((ulong)s[i + 3] << 24);

    private static ulong Fetch64(ReadOnlySpan<byte> s, int i) =>
        (ulong)s[i] | ((ulong)s[i + 1] << 8) | ((ulong)s[i + 2] << 16) | ((ulong)s[i + 3] << 24) |
        ((ulong)s[i + 4] << 32) | ((ulong)s[i + 5] << 40) | ((ulong)s[i + 6] << 48) | ((ulong)s[i + 7] << 56);

    private static ulong Hash128To64(ulong first, ulong second)
    {
        ulong a = (first ^ second) * KMul;
        a ^= a >> 47;
        ulong b = (second ^ a) * KMul;
        b ^= b >> 47;
        return b * KMul;
    }

    private static ulong HashLen16(ulong u, ulong v) => Hash128To64(u, v);

    private static ulong HashLen0To16(ReadOnlySpan<byte> s, int len)
    {
        if (len > 8)
        {
            ulong a = Fetch64(s, 0);
            ulong b = Fetch64(s, len - 8);
            return HashLen16(a, RotateByAtLeast1(b + (ulong)len, len)) ^ b;
        }
        if (len >= 4)
        {
            ulong a = Fetch32(s, 0);
            return HashLen16((ulong)len + (a << 3), Fetch32(s, len - 4));
        }
        if (len > 0)
        {
            byte a = s[0];
            byte b = s[len >> 1];
            byte c = s[len - 1];
            ulong y = a + ((ulong)b << 8);
            ulong z = (ulong)len + ((ulong)c << 2);
            return ShiftMix(y * K2 ^ z * K3) * K2;
        }
        return K2;
    }

    private static ulong HashLen17To32(ReadOnlySpan<byte> s, int len)
    {
        ulong a = Fetch64(s, 0) * K1;
        ulong b = Fetch64(s, 8);
        ulong c = Fetch64(s, len - 8) * K2;
        ulong d = Fetch64(s, len - 16) * K0;
        return HashLen16(
            Rotate(a - b, 43) + Rotate(c, 30) + d,
            a + Rotate(b ^ K3, 20) - c + (ulong)len);
    }

    private static ulong HashLen33To64(ReadOnlySpan<byte> s, int len)
    {
        ulong z = Fetch64(s, 24);
        ulong a = Fetch64(s, 0) + ((ulong)len + Fetch64(s, len - 16)) * K0;
        ulong b = Rotate(a + z, 52);
        ulong c = Rotate(a, 37);
        a += Fetch64(s, 8);
        c += Rotate(a, 7);
        a += Fetch64(s, 16);
        ulong vf = a + z;
        ulong vs = b + Rotate(a, 31) + c;
        a = Fetch64(s, 16) + Fetch64(s, len - 32);
        z = Fetch64(s, len - 8);
        b = Rotate(a + z, 52);
        c = Rotate(a, 37);
        a += Fetch64(s, len - 24);
        c += Rotate(a, 7);
        a += Fetch64(s, len - 16);
        ulong wf = a + z;
        ulong ws = b + Rotate(a, 31) + c;
        ulong r = ShiftMix((vf + ws) * K2 + (wf + vs) * K0);
        return ShiftMix(r * K0 + vs) * K2;
    }

    private static (ulong A, ulong B) WeakHashLen32WithSeedsRaw(
        ulong w, ulong x, ulong y, ulong z, ulong a, ulong b)
    {
        a += w;
        b = Rotate(b + a + z, 21);
        ulong c = a;
        a += x;
        a += y;
        b += Rotate(a, 44);
        return (a + z, b + c);
    }

    private static (ulong A, ulong B) WeakHashLen32WithSeeds(
        ReadOnlySpan<byte> s, int pos, ulong a, ulong b)
    {
        return WeakHashLen32WithSeedsRaw(
            Fetch64(s, pos), Fetch64(s, pos + 8), Fetch64(s, pos + 16), Fetch64(s, pos + 24), a, b);
    }

    private static ulong HashLen65Plus(ReadOnlySpan<byte> s, int len)
    {
        ulong x = Fetch64(s, len - 40);
        ulong y = Fetch64(s, len - 16) + Fetch64(s, len - 56);
        ulong z = HashLen16(Fetch64(s, len - 48) + (ulong)len, Fetch64(s, len - 24));
        var v = WeakHashLen32WithSeeds(s, len - 64, (ulong)len, z);
        var w = WeakHashLen32WithSeeds(s, len - 32, y + K1, x);
        x = x * K1 + Fetch64(s, 0);

        int pos = 0;
        int remaining = (len - 1) & ~63;
        while (true)
        {
            x = Rotate(x + y + v.A + Fetch64(s, pos + 8), 37) * K1;
            y = Rotate(y + v.B + Fetch64(s, pos + 48), 42) * K1;
            x ^= w.B;
            y = y + v.A + Fetch64(s, pos + 40);
            z = Rotate(z + w.A, 33) * K1;
            v = WeakHashLen32WithSeeds(s, pos, v.B * K1, x + w.A);
            w = WeakHashLen32WithSeeds(s, pos + 32, z + w.B, y + Fetch64(s, pos + 16));
            (z, x) = (x, z);
            pos += 64;
            remaining -= 64;
            if (remaining == 0) break;
        }
        return HashLen16(
            HashLen16(v.A, w.A) + ShiftMix(y) * K1 + z,
            HashLen16(v.B, w.B) + x);
    }
}
