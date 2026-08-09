namespace ScsPrefab;

/// <summary>Prefab 描述文件（.ppd）解析结果——对照 TruckLib.Models Ppd 结构（oracle，ADR-005）。</summary>
public sealed class PrefabDescriptor
{
    public required uint Version { get; init; }
    public required string SourcePath { get; init; }          // 虚拟路径（prefab_desc）
    public List<ControlNodeData> ControlNodes { get; } = new();
    public List<NavCurveData> NavCurves { get; } = new();
    public List<SemaphoreData> Semaphores { get; } = new();
    public List<NavNodeData> NavNodes { get; } = new();
    public List<IntersectionData> Intersections { get; } = new();
    // Signs/SpawnPoints/MapPoints/TriggerPoints 与导航语义无关，解析时跳过字节即可
}

public sealed class ControlNodeData
{
    public required float X { get; init; }
    public required float Y { get; init; }
    public required float Z { get; init; }
    public required float DirX { get; init; }
    public required float DirY { get; init; }
    public required float DirZ { get; init; }
    /// <summary>输入/输出线索引（-1 = 无）——指向 NavCurves。</summary>
    public int[] InputLines { get; } = new int[8];
    public int[] OutputLines { get; } = new int[8];
}

/// <summary>导航曲线（NavCurve）：一条 AI/GPS 行车线。P1-04 核心数据。</summary>
public sealed class NavCurveData
{
    public required string Name { get; set; }
    public required uint Flags { get; set; }
    /// <summary>(EndNode, EndLane, StartNode, StartLane)——曲线端点绑定的 prefab node（连接外部道路）。</summary>
    public required byte EndNode { get; set; }
    public required byte EndLane { get; set; }
    public required byte StartNode { get; set; }
    public required byte StartLane { get; set; }
    public required float StartX { get; set; }
    public required float StartY { get; set; }
    public required float StartZ { get; set; }
    public required float EndX { get; set; }
    public required float EndY { get; set; }
    public required float EndZ { get; set; }
    public required float Length { get; set; }
    public int[] NextLines { get; } = new int[4];
    public int[] PreviousLines { get; } = new int[4];
    public required int NextCount { get; set; }
    public required int PreviousCount { get; set; }
    public required int SemaphoreId { get; set; }      // -1 = 无
    public required string TrafficRule { get; set; }
    public required uint NavNodeIndex { get; set; }    // 0xFFFFFFFF = 无

    // —— 标志位语义（NavCurve.Flags，TruckLib.Models NavCurve）——
    public int Blinker => (int)((Flags >> 2) & 0b111);              // 0=none,1=left,2=right,4=both(?)——见 Enums
    public int AllowedVehicles => (int)((Flags >> 5) & 0b11);       // 0=car,1=truck,2=bus,3=all(?)——见 Enums
    public bool LowProbability => (Flags & (1u << 13)) != 0;
    public bool LimitDisplacement => (Flags & (1u << 14)) != 0;
    public bool AdditivePriority => (Flags & (1u << 15)) != 0;
    public int PriorityModifier => (int)((Flags >> 16) & 0b1111);   // nibble

    public bool IsEntry => PreviousCount == 0;   // 无前驱 = 从 prefab 边界进入
    public bool IsExit => NextCount == 0;        // 无后继 = 通向 prefab 边界
    public bool IsConnector => IsEntry || IsExit;
}

/// <summary>导航节点（NavNode）：物理/AI 节点，连接多条曲线。</summary>
public sealed class NavNodeData
{
    public required byte Type { get; init; }          // 0=Physical, 1=Ai
    public required ushort Index { get; init; }
    public List<NavNodeConnectionData> Connections { get; } = new();
}

public sealed class NavNodeConnectionData
{
    public required ushort TargetNodeIndex { get; init; }
    public required float Length { get; init; }
    public List<ushort> CurveIndices { get; } = new();
}

/// <summary>信号灯定位器（Semaphore）：curve 的 SemaphoreId 索引此数组。
/// Profile 可能为哈希 token（含 '.' 等字符时不可 base-38 解码，值存于 TokenRaw）。</summary>
public sealed class SemaphoreData
{
    public required string Profile { get; init; }
    public required ulong TokenRaw { get; init; }      // profile token 原始 u64（哈希时 Profile = "&0x…"）
    public required uint Type { get; init; }          // SemaphoreType 枚举
    public required float X { get; init; }
    public required float Y { get; init; }
    public required float Z { get; init; }
    public required float Rx { get; init; }
    public required float Ry { get; init; }
    public required float Rz { get; init; }
    public required float Rw { get; init; }
    public required uint SemaphoreId { get; init; }
    public required float Ix { get; init; }
    public required float Iy { get; init; }
    public required float Iz { get; init; }
    public required float Iw { get; init; }
    public required float CycleDelay { get; init; }
}

/// <summary>Intersection：曲线上的路口标注（优先级/规则区域）。</summary>
public sealed class IntersectionData
{
    public required uint CurveId { get; init; }
    public required float Position { get; init; }
    public required float Radius { get; init; }
    public required uint Flags { get; init; }
}
