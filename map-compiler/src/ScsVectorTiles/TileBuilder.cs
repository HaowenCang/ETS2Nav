using ScsMapModel;
using ScsSector;

namespace ScsVectorTiles;

/// <summary>
/// 矢量瓦片生成器（P1 计划 §105）：SemanticMap + sectors → map.pmtiles。
/// 图层：road（LineString）/junction（Point）/city（Point）/poi（Point）。
/// 坐标：游戏 (x,z) → 经纬度（x/111320、z/111320——与 graph-debugger 一致）。
/// </summary>
public static class TileBuilder
{
    public static void Build(SemanticMap map, IEnumerable<SectorFile> sectors,
        List<PoiEntry> pois, string outPath, int minZoom = 6, int maxZoom = 11)
    {
        // 节点索引（一次构建——road 循环逐 zoom 复用）
        var nodeIndex = sectors.SelectMany(s => s.Nodes)
            .GroupBy(n => n.Uid).ToDictionary(g => g.Key, g => g.First());
        // 几何数据（road 段在 zoom 循环内逐层处理——先建 junction/city/poi 索引）
        var junctions = map.Junctions.Select(j => (j, new Dictionary<string, object>
        {
            ["prefab"] = j.PrefabToken,
            ["movements"] = j.Movements.Count,
        })).ToList();
        var poiTags = pois.Select(p => (p, new Dictionary<string, object>
        {
            ["type"] = p.Type.ToString(),
            ["name"] = p.Name,
        })).ToList();

        var tiles = new Dictionary<long, byte[]>();
        var minLon = double.MaxValue; var minLat = double.MaxValue; var maxLon = double.MinValue; var maxLat = double.MinValue;

        for (int z = minZoom; z <= maxZoom; z++)
        {
            // 收集本 zoom 的瓦片内容
            var tileFeatures = new Dictionary<(int X, int Y), Dictionary<string, List<MvtEncoder.Feature>>>();
            void AddToTile(double lng, double lat, MvtEncoder.Feature f, string layer)
            {
                var (tx, ty) = MvtEncoder.TileXY(lng, lat, z);
                if (!tileFeatures.TryGetValue((tx, ty), out var layers))
                    tileFeatures[(tx, ty)] = layers = new Dictionary<string, List<MvtEncoder.Feature>>();
                if (!layers.TryGetValue(layer, out var list))
                    layers[layer] = list = new List<MvtEncoder.Feature>();
                list.Add(f);
                minLon = Math.Min(minLon, lng); minLat = Math.Min(minLat, lat);
                maxLon = Math.Max(maxLon, lng); maxLat = Math.Max(maxLat, lat);
            }
            // roads（线段放入中点所在瓦片；跨瓦片线段不裁剪——调试级简化）
            foreach (var r in map.Roads)
            {
                if (!nodeIndex.TryGetValue(r.Node0, out var a) || !nodeIndex.TryGetValue(r.Node1, out var b)) continue;
                var (lng0, lat0) = MvtEncoder.ToLngLat(a.X, a.Z);
                var (lng1, lat1) = MvtEncoder.ToLngLat(b.X, b.Z);
                var (px0, py0) = MvtEncoder.Project(lng0, lat0, z);
                var (px1, py1) = MvtEncoder.Project(lng1, lat1, z);
                // 线段放入其中点所在瓦片（简化；跨瓦片线段不裁剪）
                var (tx, ty) = MvtEncoder.TileXY((lng0 + lng1) / 2, (lat0 + lat1) / 2, z);
                int ox = tx * MvtEncoder.Extent, oy = ty * MvtEncoder.Extent;
                var f = new MvtEncoder.Feature
                {
                    Type = 2,
                    Points = new[] { new[] { px0 - ox, py0 - oy }, new[] { px1 - ox, py1 - oy } },
                    Tags = new Dictionary<string, object>
                    {
                        ["kind"] = r.RoadLook,
                        ["speed_limit"] = r.SpeedLimit,
                        ["one_way"] = r.Direction is RoadDirection.ForwardOnly or RoadDirection.BackwardOnly ? "yes" : "no",
                    },
                };
                if (!tileFeatures.TryGetValue((tx, ty), out var layers))
                    tileFeatures[(tx, ty)] = layers = new Dictionary<string, List<MvtEncoder.Feature>>();
                if (!layers.TryGetValue("road", out var list))
                    layers["road"] = list = new List<MvtEncoder.Feature>();
                list.Add(f);
                minLon = Math.Min(minLon, Math.Min(lng0, lng1)); minLat = Math.Min(minLat, Math.Min(lat0, lat1));
                maxLon = Math.Max(maxLon, Math.Max(lng0, lng1)); maxLat = Math.Max(maxLat, Math.Max(lat0, lat1));
            }
            // junctions/cities/poi（点）
            foreach (var j in junctions)
            {
                var n = j.j.NodeUids.FirstOrDefault();
                if (n == 0 || !nodeIndex.TryGetValue(n, out var jn)) continue;
                var (lng, lat) = MvtEncoder.ToLngLat(jn.X, jn.Z);
                var (px, py) = MvtEncoder.Project(lng, lat, z);
                var (tx, ty) = MvtEncoder.TileXY(lng, lat, z);
                AddPoint(tileFeatures, tx, ty, px - tx * MvtEncoder.Extent, py - ty * MvtEncoder.Extent, "junction", j.Item2);
            }
            // cities（用 CityItem.NodeUid——SemanticCity 未填 NodeUids）
            var cityNodes = sectors.SelectMany(s => s.Items.OfType<CityItem>())
                .GroupBy(c => c.City).Select(g => g.First()).ToList();
            foreach (var c in cityNodes)
            {
                if (!nodeIndex.TryGetValue(c.NodeUid, out var n)) continue;
                var (lng, lat) = MvtEncoder.ToLngLat(n.X, n.Z);
                var (px, py) = MvtEncoder.Project(lng, lat, z);
                var (tx, ty) = MvtEncoder.TileXY(lng, lat, z);
                AddPoint(tileFeatures, tx, ty, px - tx * MvtEncoder.Extent, py - ty * MvtEncoder.Extent, "city", new Dictionary<string, object> { ["name"] = c.City });
            }
            foreach (var p in poiTags)
            {
                var (lng, lat) = MvtEncoder.ToLngLat(p.p.X, p.p.Z);
                var (px, py) = MvtEncoder.Project(lng, lat, z);
                var (tx, ty) = MvtEncoder.TileXY(lng, lat, z);
                AddPoint(tileFeatures, tx, ty, px - tx * MvtEncoder.Extent, py - ty * MvtEncoder.Extent, "poi", p.Item2);
            }
            // 编码瓦片
            foreach (var ((tx, ty), layers) in tileFeatures)
            {
                if (layers.Count == 0) continue;
                var mvt = MvtEncoder.EncodeTile(layers);
                tiles[PmtilesWriter.TileId(z, tx, ty)] = mvt;
            }
        }

        var center = MvtEncoder.ToLngLat(0, 0);
        PmtilesWriter.Write(outPath, tiles, (maxZoom, center.Lng, center.Lat), minZoom, maxZoom,
            (minLon, minLat, maxLon, maxLat));
    }

    private static void AddPoint(Dictionary<(int, int), Dictionary<string, List<MvtEncoder.Feature>>> tiles,
        int tx, int ty, double px, double py, string layer, Dictionary<string, object> tags)
    {
        if (!tiles.TryGetValue((tx, ty), out var layers))
            tiles[(tx, ty)] = layers = new Dictionary<string, List<MvtEncoder.Feature>>();
        if (!layers.TryGetValue(layer, out var list))
            layers[layer] = list = new List<MvtEncoder.Feature>();
        list.Add(new MvtEncoder.Feature { Type = 1, Points = new[] { new[] { px, py } }, Tags = tags });
    }

}
