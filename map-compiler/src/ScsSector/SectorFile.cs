// SCS sector（.base/.aux/.snd）解析器。
// 结构依据 map-docs wiki（vendor/ref/wiki）与 TruckLib 行为对照。
// .base 文件布局：Header + u32 item_count + items（u32 type + base part）
//   + u32 node_count + nodes + u32 vis_area_count + u64[] vis_area uids
// 注意：item 的 data part 位于独立 .data 文件，P0 不解析。
// MIT License — ETS2Nav 项目

namespace ScsSector;

public enum ItemType
{
    Terrain = 1, Buildings = 2, Road = 3, Prefab = 4, Model = 5, Company = 6,
    Service = 7, CutPlane = 8, Mover = 9, EnvironmentArea = 11, City = 12,
    Hinge = 13, Parking = 14, AnimatedModel = 15, MapOverlay = 18, Ferry = 19,
    Sound = 21, Garage = 22, CameraPoint = 23, Walker = 28, Trigger = 34,
    FuelPump = 35, Sign = 36, BusStop = 37, TrafficArea = 38, BezierPatch = 39,
    Compound = 40, Trajectory = 41, MapArea = 42, FarModel = 43, Curve = 44,
    CameraPath = 45, Cutscene = 46, Hookup = 47, VisibilityArea = 48, Gate = 49,
}

public class MapItem
{
    public required ItemType Type { get; init; }
    public required ulong Uid { get; init; }
    public uint Flags { get; init; }
    public int ViewDistance { get; init; }   // 米（原始值 ×10）
}

public sealed class RoadItem : MapItem
{
    public required string RoadLook { get; init; }
    public required string RightLanes { get; init; }
    public required string LeftLanes { get; init; }
    public required ulong Node0 { get; init; }
    public required ulong Node1 { get; init; }
    public double Length { get; init; }
    public bool LeftHandTraffic { get; init; }
    public bool IsCityRoad { get; init; }
    public bool NoAiVehicles { get; init; }
    public bool GpsAvoid { get; init; }
    public bool Secret { get; init; }
}

public sealed class PrefabItem : MapItem
{
    public required string Model { get; init; }
    public required string Variant { get; init; }
    public required string[] AdditionalParts { get; init; }
    public required ulong[] NodeUids { get; init; }
    public required ulong[] SlaveUids { get; init; }
    public ulong FerryLinkUid { get; init; }
    public ushort OriginIndex { get; init; }
    /// <summary>semaphore profile 名（如 tr_sem_prof.cr_1x1）；空串=无信号灯。</summary>
    public required string SemaphoreProfile { get; init; }
    public bool CustomSemaphores { get; init; }
    public bool LeftHandTraffic { get; init; }
}

public sealed class CityItem : MapItem
{
    public required string City { get; init; }
    public double Width { get; init; }
    public double Height { get; init; }
    public required ulong NodeUid { get; init; }
}

public sealed class CompanyItem : MapItem
{
    public required string CityName { get; init; }
    public required ulong LinkedPrefabUid { get; init; }
    public required string CompanyName { get; init; }
    public required ulong MainNodeUid { get; init; }
    public required ulong[] SpawnNodeUids { get; init; }
    public required uint[] SpawnNodeFlags { get; init; }
}

public sealed class ServiceItem : MapItem
{
    public byte SpawnPointType { get; init; }
    public required ulong NodeUid { get; init; }
    public required ulong PrefabLinkUid { get; init; }
    public required ulong[] NodeUids { get; init; }
}

public sealed class FerryItem : MapItem
{
    public required string Port { get; init; }
    public required ulong PrefabLinkUid { get; init; }
    public required ulong NodeUid { get; init; }
    public bool IsTrain { get; init; }
}

public sealed class FuelPumpItem : MapItem
{
    public required ulong NodeUid { get; init; }
    public required ulong PrefabLinkUid { get; init; }
}

public sealed class GarageItem : MapItem
{
    public required string CityName { get; init; }
    public uint BuyMode { get; init; }
    public required ulong NodeUid { get; init; }
    public required ulong PrefabLinkUid { get; init; }
}

public sealed class SignItem : MapItem
{
    public required string SignModel { get; init; }
    public required ulong NodeUid { get; init; }
    public required string Look { get; init; }
    public required string Variant { get; init; }
    public bool FollowRoadDirection { get; init; }
}

public sealed class BusStopItem : MapItem
{
    public required string CityName { get; init; }
    public required ulong PrefabLinkUid { get; init; }
    public required ulong NodeUid { get; init; }
}

public sealed class CutPlaneItem : MapItem
{
    public required ulong[] NodeUids { get; init; }
}

public sealed class CutsceneItem : MapItem
{
    public required string[] Tags { get; init; }
    public required ulong NodeUid { get; init; }
}

public sealed class MapAreaItem : MapItem
{
    public required ulong[] NodeUids { get; init; }
    public uint Color { get; init; }
}

public sealed class MapOverlayItem : MapItem
{
    public required string Look { get; init; }
    public required ulong NodeUid { get; init; }
}

public sealed class TrafficAreaItem : MapItem
{
    public required string[] Tags { get; init; }
    public required ulong[] NodeUids { get; init; }
    public required string Rule { get; init; }
    public float Range { get; init; }
}

public sealed class TriggerItem : MapItem
{
    public required string[] Tags { get; init; }
    public required ulong[] NodeUids { get; init; }
    public float Range { get; init; }   // 仅单节点 trigger
}

public sealed class VisibilityAreaItem : MapItem
{
    public required ulong NodeUid { get; init; }
    public float Width { get; init; }
    public float Height { get; init; }
    public required ulong[] ChildrenUids { get; init; }
}

public sealed class TrajectoryItem : MapItem
{
    public required ulong[] NodeUids { get; init; }
    public required string AccessRule { get; init; }
}

public sealed class MapNode
{
    public required ulong Uid { get; init; }
    public required double X { get; init; }
    public required double Y { get; init; }
    public required double Z { get; init; }
    public required ulong BackwardItemUid { get; init; }
    public required ulong ForwardItemUid { get; init; }
    public uint Flags { get; init; }
}

public sealed class SectorFile
{
    public uint CoreMapVersion { get; init; }
    public required string GameId { get; init; }
    public uint GameMapVersion { get; init; }
    public required List<MapItem> Items { get; init; }
    public required List<MapNode> Nodes { get; init; }
    public required List<ulong> VisibilityAreaUids { get; init; }

    public IEnumerable<RoadItem> Roads => Items.OfType<RoadItem>();
    public IEnumerable<PrefabItem> Prefabs => Items.OfType<PrefabItem>();

    /// <summary>读取 .base/.aux/.snd 文件。</summary>
    public static SectorFile Read(string path)
    {
        using var fs = File.OpenRead(path);
        using var r = new BinaryReader(fs);
        return Read(fs, r);
    }

    /// <summary>从已定位的流读取（供调试/增量解析）。</summary>
    internal static SectorFile Read(Stream fs, BinaryReader r)
    {
        uint version = r.ReadUInt32();
        string gameId = r.ReadToken();
        uint gameMapVersion = r.ReadUInt32();

        uint itemCount = r.ReadUInt32();
        var items = new List<MapItem>((int)itemCount);
        for (int i = 0; i < itemCount; i++)
        {
            var type = (ItemType)r.ReadInt32();
            items.Add(ReadItem(r, type));
        }

        uint nodeCount = r.ReadUInt32();
        var nodes = new List<MapNode>((int)nodeCount);
        for (int i = 0; i < nodeCount; i++)
        {
            ulong uid = r.ReadUInt64();
            var (x, y, z) = r.ReadFixed3();
            r.ReadQuaternion(); // 旋转（P0 不需要）
            ulong backward = r.ReadUInt64();
            ulong forward = r.ReadUInt64();
            uint flags = r.ReadUInt32();
            nodes.Add(new MapNode { Uid = uid, X = x, Y = y, Z = z, BackwardItemUid = backward, ForwardItemUid = forward, Flags = flags });
        }

        uint visCount = r.ReadUInt32();
        var vis = new List<ulong>((int)visCount);
        for (int i = 0; i < visCount; i++) vis.Add(r.ReadUInt64());

        return new SectorFile
        {
            CoreMapVersion = version,
            GameId = gameId,
            GameMapVersion = gameMapVersion,
            Items = items,
            Nodes = nodes,
            VisibilityAreaUids = vis,
        };
    }

    /// <summary>供调试：读取单个 item（不依赖文件路径）。</summary>
    public static MapItem DebugReadItem(BinaryReader r, ItemType type) => ReadItem(r, type);

    private static MapItem ReadItem(BinaryReader r, ItemType type)
    {
        // kdop_item：uid + 5×min + 5×max（float）+ flags u32 + view_dist u8
        ulong uid = r.ReadUInt64();
        for (int i = 0; i < 10; i++) r.ReadSingle();
        uint flags = r.ReadUInt32();
        int view = r.ReadByte() * 10;

        return type switch
        {
            ItemType.Road => ReadRoad(r, uid, flags, view),
            ItemType.Prefab => ReadPrefab(r, uid, flags, view),
            ItemType.City => ReadCity(r, uid, flags, view),
            ItemType.Company => ReadCompany(r, uid, flags, view),
            ItemType.Service => ReadService(r, uid, flags, view),
            ItemType.Ferry => ReadFerry(r, uid, flags, view),
            ItemType.FuelPump => ReadFuelPump(r, uid, flags, view),
            ItemType.Garage => ReadGarage(r, uid, flags, view),
            ItemType.Sign => ReadSign(r, uid, flags, view),
            ItemType.BusStop => ReadBusStop(r, uid, flags, view),
            ItemType.CutPlane => ReadCutPlane(r, uid, flags, view),
            ItemType.Cutscene => ReadCutscene(r, uid, flags, view),
            ItemType.MapArea => ReadMapArea(r, uid, flags, view),
            ItemType.MapOverlay => ReadMapOverlay(r, uid, flags, view),
            ItemType.TrafficArea => ReadTrafficArea(r, uid, flags, view),
            ItemType.Trigger => ReadTrigger(r, uid, flags, view),
            ItemType.VisibilityArea => ReadVisibilityArea(r, uid, flags, view),
            ItemType.Trajectory => ReadTrajectory(r, uid, flags, view),
            ItemType.Terrain => ReadTerrain(r, uid, flags, view),
            ItemType.Model => ReadModel(r, uid, flags, view),
            ItemType.Buildings => ReadBuildings(r, uid, flags, view),
            ItemType.Curve => ReadCurve(r, uid, flags, view),
            ItemType.BezierPatch => ReadBezierPatch(r, uid, flags, view),
            _ => new MapItem { Type = type, Uid = uid, Flags = flags, ViewDistance = view },
        };
    }

    /// <summary>ActionBase 数组：u32 数量（0xFFFFFFFF=无）；每个 action：[可选 Name token] + u32 numParams（0xFFFFFFFF=无参数）→ numParams×float → u32 strCount → (u64 len + bytes)×n → u32 tagCount × token → float range → u32 flags。</summary>
    private static void SkipActions(BinaryReader r, bool withName = false)
    {
        uint actionCount = r.ReadUInt32();
        if (actionCount == uint.MaxValue) return;
        for (int a = 0; a < actionCount; a++)
        {
            if (withName) r.ReadToken();
            uint np = r.ReadUInt32();
            if (np == uint.MaxValue) continue;
            for (int i = 0; i < np; i++) r.ReadSingle();
            uint strCount = r.ReadUInt32();
            for (int i = 0; i < strCount; i++)
            {
                ulong len = r.ReadUInt64();
                r.ReadBytes((int)len);
            }
            uint tagCount = r.ReadUInt32();
            for (int i = 0; i < tagCount; i++) r.ReadToken();
            r.ReadSingle();  // target range
            r.ReadUInt32();  // action flags
        }
    }

    private static string ReadPascalString(BinaryReader r)
    {
        ulong len = r.ReadUInt64();
        return System.Text.Encoding.UTF8.GetString(r.ReadBytes((int)len));
    }

    private static RoadItem ReadRoad(BinaryReader r, ulong uid, uint flags, int view)
    {
        // kdop flags 4 字节已读；随后：road_flags 4 字节（rflag1+dlc+rflag3+rflag4）
        r.ReadUInt32();          // road_flags
        string roadLook = r.ReadToken();
        string rightLanes = r.ReadToken();
        string leftLanes = r.ReadToken();
        string rightTmpl = r.ReadToken();
        string leftTmpl = r.ReadToken();
        string rightEdgeRight = r.ReadToken();
        string rightEdgeLeft = r.ReadToken();
        string leftEdgeRight = r.ReadToken();
        string leftEdgeLeft = r.ReadToken();
        r.ReadToken(); r.ReadSingle();   // right terrain profile + coef
        r.ReadToken(); r.ReadSingle();   // left terrain profile + coef
        string rightLook = r.ReadToken();
        string leftLook = r.ReadToken();
        r.ReadToken();                   // road material
        for (int i = 0; i < 3; i++)      // railings ×3（每侧 token+s16）
        {
            r.ReadToken(); r.ReadInt16();
            r.ReadToken(); r.ReadInt16();
        }
        r.ReadInt32();                   // right road height
        r.ReadInt32();                   // left road height
        ulong node0 = r.ReadUInt64();
        ulong node1 = r.ReadUInt64();
        double length = r.ReadSingle();

        return new RoadItem
        {
            Type = ItemType.Road, Uid = uid, Flags = flags, ViewDistance = view,
            RoadLook = roadLook, RightLanes = rightLanes, LeftLanes = leftLanes,
            Node0 = node0, Node1 = node1, Length = length,
            LeftHandTraffic = (flags & (1u << 15)) != 0,
            IsCityRoad = (flags & (1u << 19)) != 0,
            NoAiVehicles = (flags & (1u << 22)) != 0,
            Secret = (flags & (1u << 16)) != 0,
            GpsAvoid = (flags & (1u << 28)) != 0,
        };
    }

    private static PrefabItem ReadPrefab(BinaryReader r, ulong uid, uint flags, int view)
    {
        string model = r.ReadToken();
        string variant = r.ReadToken();
        string[] addParts = r.ReadTokenArray();
        ulong[] nodeUids = r.ReadUidArray();
        ulong[] slaveUids = r.ReadUidArray();
        ulong ferryLink = r.ReadUInt64();
        ushort origin = r.ReadUInt16();
        for (int i = 0; i < nodeUids.Length; i++)
        {
            r.ReadToken(); r.ReadSingle();   // node terrain profile + coef
        }
        string semaphoreProfile = r.ReadToken();

        return new PrefabItem
        {
            Type = ItemType.Prefab, Uid = uid, Flags = flags, ViewDistance = view,
            Model = model, Variant = variant, AdditionalParts = addParts,
            NodeUids = nodeUids, SlaveUids = slaveUids, FerryLinkUid = ferryLink,
            OriginIndex = origin, SemaphoreProfile = semaphoreProfile,
            CustomSemaphores = (flags & (1u << 1)) != 0,
            LeftHandTraffic = (flags & (1u << 22)) != 0,
        };
    }

    private static CityItem ReadCity(BinaryReader r, ulong uid, uint flags, int view)
    {
        string city = r.ReadToken();
        double width = r.ReadSingle();
        double height = r.ReadSingle();
        ulong nodeUid = r.ReadUInt64();
        return new CityItem
        {
            Type = ItemType.City, Uid = uid, Flags = flags, ViewDistance = view,
            City = city, Width = width, Height = height, NodeUid = nodeUid,
        };
    }

    private static CompanyItem ReadCompany(BinaryReader r, ulong uid, uint flags, int view)
    {
        // economy_link_item：kdop + linked_city_name token + linked_item_uid u64
        string cityName = r.ReadToken();
        ulong linkedPrefab = r.ReadUInt64();
        string companyName = r.ReadToken();
        ulong mainNode = r.ReadUInt64();
        ulong[] spawnNodes = r.ReadUidArray();
        // flags 数量 = spawn node 数量（无独立 count 字段）
        var spawnFlags = new uint[spawnNodes.Length];
        for (int i = 0; i < spawnNodes.Length; i++) spawnFlags[i] = r.ReadUInt32();
        return new CompanyItem
        {
            Type = ItemType.Company, Uid = uid, Flags = flags, ViewDistance = view,
            CityName = cityName, LinkedPrefabUid = linkedPrefab, CompanyName = companyName,
            MainNodeUid = mainNode, SpawnNodeUids = spawnNodes, SpawnNodeFlags = spawnFlags,
        };
    }

    private static ServiceItem ReadService(BinaryReader r, ulong uid, uint flags, int view)
    {
        ulong nodeUid = r.ReadUInt64();
        ulong prefabLink = r.ReadUInt64();
        ulong[] nodeUids = r.ReadUidArray();
        return new ServiceItem
        {
            Type = ItemType.Service, Uid = uid, Flags = flags, ViewDistance = view,
            SpawnPointType = (byte)(flags & 0xFF), NodeUid = nodeUid,
            PrefabLinkUid = prefabLink, NodeUids = nodeUids,
        };
    }

    private static FerryItem ReadFerry(BinaryReader r, ulong uid, uint flags, int view)
    {
        string port = r.ReadToken();
        ulong prefabLink = r.ReadUInt64();
        ulong nodeUid = r.ReadUInt64();
        r.ReadSingle(); r.ReadSingle(); r.ReadSingle();   // unload offset
        return new FerryItem
        {
            Type = ItemType.Ferry, Uid = uid, Flags = flags, ViewDistance = view,
            Port = port, PrefabLinkUid = prefabLink, NodeUid = nodeUid,
            IsTrain = (flags & 1u) != 0,
        };
    }

    private static FuelPumpItem ReadFuelPump(BinaryReader r, ulong uid, uint flags, int view)
    {
        ulong nodeUid = r.ReadUInt64();
        ulong prefabLink = r.ReadUInt64();
        r.ReadUidArray();   // unused node uids
        return new FuelPumpItem
        {
            Type = ItemType.FuelPump, Uid = uid, Flags = flags, ViewDistance = view,
            NodeUid = nodeUid, PrefabLinkUid = prefabLink,
        };
    }

    private static GarageItem ReadGarage(BinaryReader r, ulong uid, uint flags, int view)
    {
        string cityName = r.ReadToken();
        uint buyMode = r.ReadUInt32();
        ulong nodeUid = r.ReadUInt64();
        ulong prefabLink = r.ReadUInt64();
        r.ReadUidArray();   // trailer spawn nodes
        return new GarageItem
        {
            Type = ItemType.Garage, Uid = uid, Flags = flags, ViewDistance = view,
            CityName = cityName, BuyMode = buyMode, NodeUid = nodeUid, PrefabLinkUid = prefabLink,
        };
    }

    private static SignItem ReadSign(BinaryReader r, ulong uid, uint flags, int view)
    {
        string signModel = r.ReadToken();
        ulong nodeUid = r.ReadUInt64();
        string look = r.ReadToken();
        string variant = r.ReadToken();
        byte boardCount = r.ReadByte();
        for (int i = 0; i < boardCount; i++)
        {
            r.ReadToken(); r.ReadToken(); r.ReadToken();   // road, city1, city2
        }
        string signTemplate = ReadPascalString(r);
        if (signTemplate.Length > 0)
        {
            // board overrides
            uint boCount = r.ReadUInt32();
            for (int i = 0; i < boCount; i++)
            {
                r.ReadToken();              // area name
                byte bf = r.ReadByte();     // flags
                if ((bf & 1) != 0) { r.ReadSByte(); r.ReadSByte(); }   // offset
                if ((bf & 2) != 0) r.ReadToken();                       // board
            }
            // sign overrides
            uint soCount = r.ReadUInt32();
            for (int i = 0; i < soCount; i++)
            {
                r.ReadUInt32();             // id
                r.ReadToken();              // area name
                uint attrCount = r.ReadUInt32();
                for (int a = 0; a < attrCount; a++)
                {
                    ushort at = r.ReadUInt16();
                    uint idx = r.ReadUInt32();
                    switch (at)
                    {
                        case 1: r.ReadSByte(); break;
                        case 2: r.ReadInt32(); break;
                        case 3: r.ReadUInt32(); break;
                        case 4: r.ReadSingle(); break;
                        case 5: ReadPascalString(r); break;
                        case 6: r.ReadUInt64(); break;
                        default: throw new SectorParseException($"未知 sign override 属性类型 {at}");
                    }
                }
            }
        }
        return new SignItem
        {
            Type = ItemType.Sign, Uid = uid, Flags = flags, ViewDistance = view,
            SignModel = signModel, NodeUid = nodeUid, Look = look, Variant = variant,
            FollowRoadDirection = (flags & (1u << 24)) != 0,
        };
    }

    private static BusStopItem ReadBusStop(BinaryReader r, ulong uid, uint flags, int view)
    {
        string cityName = r.ReadToken();
        ulong prefabLink = r.ReadUInt64();
        ulong nodeUid = r.ReadUInt64();
        return new BusStopItem
        {
            Type = ItemType.BusStop, Uid = uid, Flags = flags, ViewDistance = view,
            CityName = cityName, PrefabLinkUid = prefabLink, NodeUid = nodeUid,
        };
    }

    private static CutPlaneItem ReadCutPlane(BinaryReader r, ulong uid, uint flags, int view)
    {
        ulong[] nodeUids = r.ReadUidArray();
        return new CutPlaneItem { Type = ItemType.CutPlane, Uid = uid, Flags = flags, ViewDistance = view, NodeUids = nodeUids };
    }

    private static CutsceneItem ReadCutscene(BinaryReader r, ulong uid, uint flags, int view)
    {
        string[] tags = r.ReadTokenArray();
        ulong nodeUid = r.ReadUInt64();
        SkipActions(r);
        return new CutsceneItem { Type = ItemType.Cutscene, Uid = uid, Flags = flags, ViewDistance = view, Tags = tags, NodeUid = nodeUid };
    }

    private static MapAreaItem ReadMapArea(BinaryReader r, ulong uid, uint flags, int view)
    {
        ulong[] nodeUids = r.ReadUidArray();
        uint color = r.ReadUInt32();
        return new MapAreaItem { Type = ItemType.MapArea, Uid = uid, Flags = flags, ViewDistance = view, NodeUids = nodeUids, Color = color };
    }

    private static MapOverlayItem ReadMapOverlay(BinaryReader r, ulong uid, uint flags, int view)
    {
        string look = r.ReadToken();
        ulong nodeUid = r.ReadUInt64();
        return new MapOverlayItem { Type = ItemType.MapOverlay, Uid = uid, Flags = flags, ViewDistance = view, Look = look, NodeUid = nodeUid };
    }

    private static TrafficAreaItem ReadTrafficArea(BinaryReader r, ulong uid, uint flags, int view)
    {
        string[] tags = r.ReadTokenArray();
        ulong[] nodeUids = r.ReadUidArray();
        string rule = r.ReadToken();
        float range = r.ReadSingle();
        return new TrafficAreaItem { Type = ItemType.TrafficArea, Uid = uid, Flags = flags, ViewDistance = view, Tags = tags, NodeUids = nodeUids, Rule = rule, Range = range };
    }

    private static TriggerItem ReadTrigger(BinaryReader r, ulong uid, uint flags, int view)
    {
        string[] tags = r.ReadTokenArray();
        ulong[] nodeUids = r.ReadUidArray();
        SkipActions(r, withName: true);   // TriggerAction 含 Name token
        float range = nodeUids.Length == 1 ? r.ReadSingle() : 0;
        return new TriggerItem { Type = ItemType.Trigger, Uid = uid, Flags = flags, ViewDistance = view, Tags = tags, NodeUids = nodeUids, Range = range };
    }

    private static VisibilityAreaItem ReadVisibilityArea(BinaryReader r, ulong uid, uint flags, int view)
    {
        ulong nodeUid = r.ReadUInt64();
        float width = r.ReadSingle();
        float height = r.ReadSingle();
        ulong[] children = r.ReadUidArray();
        return new VisibilityAreaItem { Type = ItemType.VisibilityArea, Uid = uid, Flags = flags, ViewDistance = view, NodeUid = nodeUid, Width = width, Height = height, ChildrenUids = children };
    }

    private static TrajectoryItem ReadTrajectory(BinaryReader r, ulong uid, uint flags, int view)
    {
        ulong[] nodeUids = r.ReadUidArray();
        string accessRule = r.ReadToken();
        // rules
        uint ruleCount = r.ReadUInt32();
        for (int i = 0; i < ruleCount; i++)
        {
            r.ReadUInt32();      // node index
            r.ReadToken();       // rule
            uint pCount = r.ReadUInt32();
            for (int p = 0; p < pCount; p++) r.ReadSingle();
        }
        // checkpoints
        uint cpCount = r.ReadUInt32();
        for (int i = 0; i < cpCount; i++)
        {
            r.ReadToken(); r.ReadToken();   // route, checkpoint
        }
        r.ReadTokenArray();      // tags
        return new TrajectoryItem { Type = ItemType.Trajectory, Uid = uid, Flags = flags, ViewDistance = view, NodeUids = nodeUids, AccessRule = accessRule };
    }

// ---- Far Model 例外类型（被 Far Model 父级时写入 .base）----

/// <summary>Vegetation struct：token + u16 + u8 + u8 + u16 + u16。</summary>
private static void SkipVegetation(BinaryReader r)
{
    r.ReadToken(); r.ReadUInt16(); r.ReadByte(); r.ReadByte(); r.ReadUInt16(); r.ReadUInt16();
}

/// <summary>TerrainQuadData：u16 matCount + (token+u16)×n + u16 colorCount + u32×n + u16 rows + u16 cols + u32 quadCount + u32×n + u32 offCount + (u16+u16+vec3)×n + u32 normCount + (u16+u16+vec3)×n。</summary>
private static void SkipQuadData(BinaryReader r)
{
    ushort matCount = r.ReadUInt16();
    for (int i = 0; i < matCount; i++) { r.ReadToken(); r.ReadUInt16(); }
    ushort colorCount = r.ReadUInt16();
    for (int i = 0; i < colorCount; i++) r.ReadUInt32();
    r.ReadUInt16(); r.ReadUInt16();   // rows, cols
    uint quadCount = r.ReadUInt32();
    for (int i = 0; i < quadCount; i++) r.ReadUInt32();
    uint offCount = r.ReadUInt32();
    for (int i = 0; i < offCount; i++) { r.ReadUInt16(); r.ReadUInt16(); r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); }
    uint normCount = r.ReadUInt32();
    for (int i = 0; i < normCount; i++) { r.ReadUInt16(); r.ReadUInt16(); r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); }
}

/// <summary>VegetationSphere：vec3 + float + u32。</summary>
private static void SkipVegetationSphere(BinaryReader r)
{
    r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); r.ReadUInt32();
}

private static MapItem ReadTerrain(BinaryReader r, ulong uid, uint flags, int view)
{
    // 注意：kdop flags（kflag1-4）已由 ReadItem 读取，此处不再读
    r.ReadUInt64();          // node
    r.ReadUInt64();          // forward node
    r.ReadSingle(); r.ReadSingle(); r.ReadSingle();   // node offset
    r.ReadSingle(); r.ReadSingle(); r.ReadSingle();   // forward node offset
    r.ReadSingle();          // length
    r.ReadSingle();          // previous length
    r.ReadUInt32();          // random seed
    for (int i = 0; i < 3; i++) { r.ReadToken(); r.ReadInt16(); }   // railings
    for (int side = 0; side < 2; side++)
    {
        r.ReadUInt16();      // terrain size
        r.ReadToken();       // profile
        r.ReadSingle();      // coef
        r.ReadToken();       // prev profile
        r.ReadSingle();      // prev coef
        for (int i = 0; i < 3; i++) SkipVegetation(r);
        r.ReadUInt16(); r.ReadUInt16();   // no detail veg from/to
    }
    uint sphereCount = r.ReadUInt32();
    for (int i = 0; i < sphereCount; i++) SkipVegetationSphere(r);
    SkipQuadData(r);
    SkipQuadData(r);
    r.ReadToken(); r.ReadToken(); r.ReadToken(); r.ReadToken();   // edges + looks
    return new MapItem { Type = ItemType.Terrain, Uid = uid, Flags = flags, ViewDistance = view };
}

private static MapItem ReadModel(BinaryReader r, ulong uid, uint flags, int view)
{
    r.ReadToken();           // name
    r.ReadToken();           // look
    r.ReadToken();           // variant
    r.ReadTokenArray();      // additional parts
    r.ReadUInt64();          // node
    r.ReadSingle(); r.ReadSingle(); r.ReadSingle();   // scale
    r.ReadToken();           // terrain material
    r.ReadUInt32();          // terrain color
    r.ReadSingle();          // terrain rotation
    return new MapItem { Type = ItemType.Model, Uid = uid, Flags = flags, ViewDistance = view };
}

private static MapItem ReadBuildings(BinaryReader r, ulong uid, uint flags, int view)
{
    r.ReadToken();           // name
    r.ReadToken();           // look
    r.ReadUInt64();          // node
    r.ReadUInt64();          // forward node
    r.ReadSingle();          // length
    r.ReadUInt32();          // random seed
    r.ReadSingle();          // stretch
    uint count = r.ReadUInt32();
    for (int i = 0; i < count; i++) r.ReadSingle();
    return new MapItem { Type = ItemType.Buildings, Uid = uid, Flags = flags, ViewDistance = view };
}

private static MapItem ReadCurve(BinaryReader r, ulong uid, uint flags, int view)
{
    r.ReadUInt64();          // node
    r.ReadUInt64();          // forward node
    r.ReadUInt64();          // locator 1
    r.ReadUInt64();          // locator 2
    r.ReadSingle();          // length
    uint mask = r.ReadUInt32();
    int subcurveCount = System.Numerics.BitOperations.PopCount(mask);
    for (int i = 0; i < subcurveCount; i++)
    {
        r.ReadToken();       // model
        r.ReadUInt32();      // flags
        r.ReadUInt32();      // seed
        r.ReadSingle();      // stretch
        r.ReadSingle();      // scale
        r.ReadSingle();      // fixed step
        r.ReadToken();       // terrain material
        r.ReadUInt32();      // terrain color
        r.ReadSingle();      // terrain rotation
        r.ReadToken();       // first part
        r.ReadToken();       // last part
        r.ReadToken();       // center part variation
        r.ReadToken();       // look
        uint hoCount = r.ReadUInt32();
        for (int h = 0; h < hoCount; h++) r.ReadSingle();
        for (int h = 0; h < 5; h++) r.ReadSingle();   // initial/offset values
    }
    return new MapItem { Type = ItemType.Curve, Uid = uid, Flags = flags, ViewDistance = view };
}

private static MapItem ReadBezierPatch(BinaryReader r, ulong uid, uint flags, int view)
{
    for (int i = 0; i < 16; i++) { r.ReadSingle(); r.ReadSingle(); r.ReadSingle(); }   // 4×4 control points
    r.ReadUInt16(); r.ReadUInt16();   // tesselation
    r.ReadUInt64();          // node
    r.ReadUInt32();          // random seed
    for (int i = 0; i < 3; i++)
    {
        r.ReadToken(); r.ReadUInt16(); r.ReadByte();   // vegetation (name, density, scale)
    }
    uint sphereCount = r.ReadUInt32();
    for (int i = 0; i < sphereCount; i++) SkipVegetationSphere(r);
    SkipQuadData(r);
    return new MapItem { Type = ItemType.BezierPatch, Uid = uid, Flags = flags, ViewDistance = view };
}
}

public sealed class SectorParseException : Exception
{
    public SectorParseException(string message) : base(message) { }
}

