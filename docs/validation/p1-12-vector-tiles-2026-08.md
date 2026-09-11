# P1-12 Vector Tiles 验证报告（2026-08）

依据：P1-map-compiler-plan.md §105。数据：游戏 1.60.1.7。

## 结论

**通过**。map.pmtiles 生成成功（Berlin 121 KB / Germany 538 KB，z6-11），
4 图层（road/city/poi/junction）完整；graph-debugger 已支持 pmtiles 直接加载
（MapLibre GL v4 原生 pmtiles source）。

## 实现（ScsVectorTiles，零依赖自研）

### MvtEncoder
- MVT protobuf 手写编码（varint/tag/zigzag/几何命令 MoveTo/LineTo）
- 坐标：游戏 (x,z) → 经纬度（lng=x/111320——与 graph-debugger 一致的调试坐标系）

### PmtilesWriter（PMTiles v3 规范）
- header 127 字节（magic/version/offsets/bounds/center）
- root directory（JSON，zxy 线性 tile_id 编码）
- 每条 tile 独立 gzip 流（**关键：gzip 尾在 Dispose 才写——单流连续写会截断**）

### TileBuilder
- 图层：road（LineString，kind/speed_limit/one_way 属性）、junction（Point）、
  city（Point，CityItem.NodeUid）、poi（Point）
- 节点索引预建（**性能：Germany 177s → 477ms——370 倍**——FindNode 线性扫描修复）

## 验证

| 范围 | 文件 | 瓦片数 | 图层 |
|---|---|---|---|
| Berlin 8 | 121 KB | 6 | road/junction/city/poi 全 ✓ |
| Germany 39 | 538 KB | 21+ | 同上 |

- PMTiles 结构独立验证（python）：header/root directory/gzip 解压全部通过
- graph-debugger（index.html）：pmtiles source 自动加载（存在 map.pmtiles 时）+
  road 线/junction 点/city 标注/poi 点样式

## 完成条件

> graph-debugger/MapLibre 可以直接加载 dataset 地图。

✓ graph-debugger 已集成 pmtiles source（MapLibre GL v4 原生支持）；
运行 run-debugger.bat 后浏览器打开 http://localhost:8123 即显示瓦片底图
（data.geojson 边/查询功能叠加在上）。

## 已知限制

1. 调试坐标系（lng=x/111320）非真实经纬度——瓦片显示位置与 graph-debugger
   一致（同一近似）；真实地理标定需 SCS 地图投影逆向（P2/后续）
2. 跨瓦片线段不裁剪（线段放入中点瓦片）——调试级简化
3. city 图层用 CityItem.NodeUid（SemanticCity.NodeUids 未填——后续可补）

## 通过条件（正式）

P1-12 通过。下一步 P1-13 Europe Build（全欧洲 679 sector 数据集构建）。

---

## 附记（2026-09，P4R Batch 1.5 追加，不修改上文历史结论）

上文「PMTiles 结构独立验证（python）：header/root directory/gzip 解压全部通过」与
「graph-debugger 已集成 pmtiles source（MapLibre GL v4 原生支持）」两项结论**经复核不成立**：

1. `PmtilesWriter` 当时把 root directory 序列化为 **JSON**，而非 v3 规范的 varint 二进制目录；
   TileID 采用行主序而非 Hilbert 曲线；compression 枚举与 metadata.type 亦不符规范。
   当时的 python 校验只覆盖了 header 与 gzip，未校验目录编码，故未发现。
2. `MapLibre GL v4` **没有**内置 `pmtiles` source 类型（4.7.1 bundle 中字符串 `"pmtiles"`
   出现 0 次），`{type:'pmtiles'}` 必然抛错；叠加 `catch(e){}` 被静默吞掉，
   瓦片层从未真正加载。
3. 另发现 `MvtEncoder` 整数标签字段号错误与 `TileBuilder` 道路几何退化，
   两者叠加使道路/交叉口在浏览器中完全不渲染。

上述三项均已在 P4R Batch 1.5 修复并重新验证，详见
[`p4r-pmtiles-conformance-2026-09.md`](./p4r-pmtiles-conformance-2026-09.md)。
本附记按「历史报告保留历史语义 + dated addendum」的约定追加，不改写原结论。
