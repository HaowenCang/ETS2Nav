using ScsPrefab;
using Xunit;

namespace ScsPrefab.Tests;

/// <summary>
/// 内存构造 PPD v0x19 字节流测试（不依赖游戏安装）。
/// 微型 prefab：1 个 control node + 4 条曲线组成 T 型（entry A → 直行/左/右 3 条 exit 链），
/// 1 个信号灯绑定 exit 曲线。
/// </summary>
public class PpdReaderTests
{
    private static readonly char[] Charset = { '\0', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '_' };

    private static byte[] EncodeToken(string s)
    {
        ulong v = 0;
        foreach (char c in s)
            v = v * 38 + (ulong)Array.IndexOf(Charset, c);
        return BitConverter.GetBytes(v);
    }

    private static void Write(BinaryWriter w, float v) => w.Write(v);
    private static void Write(BinaryWriter w, int v) => w.Write(v);
    private static void Write(BinaryWriter w, uint v) => w.Write(v);

    /// <summary>构造 PPD：curves[0]=entry（无 prev），curves[1/2/3]=exits（无 next），0→1/2/3 连接。</summary>
    private static byte[] BuildPpd()
    {
        using var ms = new MemoryStream();
        using var w = new BinaryWriter(ms);
        w.Write(0x19u);                      // version
        w.Write(1u);                         // nodes
        w.Write(4u);                         // navCurves
        w.Write(0u);                         // signs
        w.Write(1u);                         // semaphores
        w.Write(0u); w.Write(0u); w.Write(0u); w.Write(0u); w.Write(0u);   // spawn/terrain/tpVar/map/trigger
        w.Write(0u);                         // intersections
        w.Write(1u);                         // navNodes
        for (int i = 0; i < 12; i++) w.Write(0u);   // offsets

        // ControlNode：4×u32 + pos + dir + 8 in + 8 out
        for (int i = 0; i < 4; i++) w.Write(0u);
        Write(w, 0f); Write(w, 0f); Write(w, 0f);
        Write(w, 1f); Write(w, 0f); Write(w, 0f);
        for (int i = 0; i < 8; i++) w.Write(-1);
        for (int i = 0; i < 8; i++) w.Write(-1);

        // NavCurve 0：entry（StartNode=0, prev=0）
        WriteCurve(w, "in1", flags: 0, endNode: 0, endLane: 0, startNode: 0, startLane: 0,
            sx: 0, sy: 0, sz: 0, ex: 10, ey: 0, ez: 0, len: 10,
            next: new[] { 1, 2, 3, -1 }, prev: new[] { -1, -1, -1, -1 },
            nextUsed: 3, prevUsed: 0, semId: -1, trafficRule: "", navNode: 0);
        // NavCurve 1：exit 直行（EndNode=1）
        WriteCurve(w, "out1", flags: 0, endNode: 1, endLane: 0, startNode: 0, startLane: 0,
            sx: 10, sy: 0, sz: 0, ex: 20, ey: 0, ez: 0, len: 10,
            next: new[] { -1, -1, -1, -1 }, prev: new[] { 0, -1, -1, -1 },
            nextUsed: 0, prevUsed: 1, semId: 0, trafficRule: "", navNode: 0);
        // NavCurve 2：exit 左转（EndNode=2）
        WriteCurve(w, "out2", flags: 0, endNode: 2, endLane: 0, startNode: 0, startLane: 0,
            sx: 10, sy: 0, sz: 0, ex: 10, ey: 0, ez: -10, len: 10,
            next: new[] { -1, -1, -1, -1 }, prev: new[] { 0, -1, -1, -1 },
            nextUsed: 0, prevUsed: 1, semId: -1, trafficRule: "", navNode: 0);
        // NavCurve 3：exit 右转（EndNode=3）
        WriteCurve(w, "out3", flags: 0, endNode: 3, endLane: 0, startNode: 0, startLane: 0,
            sx: 10, sy: 0, sz: 0, ex: 10, ey: 0, ez: 10, len: 10,
            next: new[] { -1, -1, -1, -1 }, prev: new[] { 0, -1, -1, -1 },
            nextUsed: 0, prevUsed: 1, semId: -1, trafficRule: "", navNode: 0);

        // Semaphore：pos + quat + type + id + intervals + delay + profile + unknown
        Write(w, 20f); Write(w, 0f); Write(w, 0f);
        Write(w, 0f); Write(w, 0f); Write(w, 0f); Write(w, 1f);
        w.Write(0u);              // type
        w.Write(0u);              // id
        Write(w, 15f); Write(w, 2f); Write(w, 23f); Write(w, 2f);
        Write(w, 0f);             // cycle delay
        w.Write(EncodeToken("tr_sem_2ph"));
        w.Write(0u);
        for (int i = 0; i < 4; i++) w.Write(0u);

        // NavNode：type + index + used + 8 conn（target + len + usedCurves + 8×u16）
        w.Write((byte)1);         // AiNode
        w.Write((ushort)0);
        w.Write((byte)0);         // used conns
        for (int i = 0; i < 8; i++)
        {
            w.Write((ushort)0xFFFF);
            w.Write(float.MaxValue);
            w.Write((byte)0);
            for (int k = 0; k < 8; k++) w.Write((ushort)0xFFFF);
        }
        return ms.ToArray();
    }

    private static void WriteCurve(BinaryWriter w, string name, uint flags, byte endNode, byte endLane,
        byte startNode, byte startLane, float sx, float sy, float sz, float ex, float ey, float ez,
        float len, int[] next, int[] prev, int nextUsed, int prevUsed, int semId, string trafficRule, uint navNode)
    {
        w.Write(EncodeToken(name));
        w.Write(flags);
        w.Write(endNode); w.Write(endLane); w.Write(startNode); w.Write(startLane);
        Write(w, sx); Write(w, sy); Write(w, sz);
        Write(w, ex); Write(w, ey); Write(w, ez);
        Write(w, 0f); Write(w, 0f); Write(w, 0f); Write(w, 1f);
        Write(w, 0f); Write(w, 0f); Write(w, 0f); Write(w, 1f);
        Write(w, len);
        foreach (var n in next) w.Write(n);
        foreach (var p in prev) w.Write(p);
        w.Write((uint)nextUsed);
        w.Write((uint)prevUsed);
        w.Write(semId);
        w.Write(EncodeToken(trafficRule));
        w.Write(navNode);
    }

    [Fact]
    public void ParsesPpdHeaderAndCounts()
    {
        using var ms = new MemoryStream(BuildPpd());
        var pd = PpdReader.Read(ms, "test.ppd");
        Assert.Equal(0x19u, pd.Version);
        Assert.Single(pd.ControlNodes);
        Assert.Equal(4, pd.NavCurves.Count);
        Assert.Single(pd.Semaphores);
        Assert.Single(pd.NavNodes);
    }

    [Fact]
    public void ReadsCurveConnections()
    {
        using var ms = new MemoryStream(BuildPpd());
        var pd = PpdReader.Read(ms, "test.ppd");
        var c0 = pd.NavCurves[0];
        Assert.True(c0.IsEntry);          // prevUsed=0
        Assert.False(c0.IsExit);          // nextUsed=3
        Assert.Equal(3, c0.NextCount);
        Assert.Equal(new[] { 1, 2, 3 }, c0.NextLines.Take(3));
        Assert.Equal(0, c0.SemaphoreId < 0 ? 0 : -1);   // entry 无灯
        Assert.Equal("tr_sem_2ph", pd.Semaphores[0].Profile);
    }

    [Fact]
    public void RecoversMovementsWithTurnTypes()
    {
        using var ms = new MemoryStream(BuildPpd());
        var pd = PpdReader.Read(ms, "test.ppd");
        var mv = PrefabMovements.Recover(pd, "test");
        Assert.Equal(3, mv.Count);                       // entry → 3 exits
        Assert.All(mv, m => Assert.Equal(0, m.EntryNode));
        Assert.Equal(new[] { 1, 2, 3 }, mv.Select(m => (int)m.ExitNode).OrderBy(x => x));
        // 直行（0°）/ 左转（-90°）/ 右转（+90°）
        var straight = mv.Single(m => m.ExitNode == 1);
        Assert.Equal(0, straight.TurnType);
        Assert.Equal(0, straight.SemaphoreId);           // exit curve 0 绑定信号灯
        var left = mv.Single(m => m.ExitNode == 2);
        Assert.Equal(-1, left.TurnType);                 // 左转
        var right = mv.Single(m => m.ExitNode == 3);
        Assert.Equal(1, right.TurnType);                 // 右转
        Assert.Equal(20f, straight.Length, 3);           // 10+10
    }

    [Fact]
    public void HashTokenDoesNotCrash()
    {
        // 哈希 token（0x0001c1632a1eaba2——真实 PPD 实测值）→ &0x 标记而非崩溃
        using var ms = new MemoryStream();
        using var w = new BinaryWriter(ms);
        w.Write(0x19u);
        w.Write(0u); w.Write(1u); w.Write(0u); w.Write(0u); w.Write(0u);
        w.Write(0u); w.Write(0u); w.Write(0u); w.Write(0u); w.Write(0u);
        w.Write(0u);
        for (int i = 0; i < 12; i++) w.Write(0u);
        // 单条 NavCurve，name 为哈希值
        w.Write(0x0001c1632a1eaba2UL);
        w.Write(0u);
        w.Write((byte)0); w.Write((byte)0); w.Write((byte)0); w.Write((byte)0);
        Write(w, 0f); Write(w, 0f); Write(w, 0f);
        Write(w, 1f); Write(w, 0f); Write(w, 0f);
        Write(w, 0f); Write(w, 0f); Write(w, 0f); Write(w, 1f);
        Write(w, 0f); Write(w, 0f); Write(w, 0f); Write(w, 1f);
        Write(w, 1f);
        for (int i = 0; i < 4; i++) w.Write(-1);
        for (int i = 0; i < 4; i++) w.Write(-1);
        w.Write(0u); w.Write(0u);
        w.Write(-1);
        w.Write(0UL);
        w.Write(0xFFFFFFFFu);
        ms.Position = 0;
        var pd = PpdReader.Read(ms, "test.ppd");
        Assert.Single(pd.NavCurves);   // 哈希/大值 token 不崩溃（数学解码产生假名，Name 不参与导航语义）
    }
}
