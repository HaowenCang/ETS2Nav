# P4R Batch 1.5 — PMTiles v3 规范符合性修复（2026-09）

依据：`docs/dev/P4R-HARDENING-TASK.md` §12 例外条款——正式 `PmtilesWriter` 被确认为
P4R blocker，允许仅针对 PMTiles 生产链做最小范围 map-compiler 修改。

基线 commit：`1d22cf6`（Batch 1 checkpoint）
范围：`map-compiler/src/ScsVectorTiles/**`、`map-compiler/tests/ScsVectorTiles.Tests/**`、
`map-compiler/tools/PmtilesFixture/**`、nav-server 的 HTTP Range/416 语义、
`tools/ets2nav-web/scripts/verify-pmtiles.mjs`。

## 1. 问题

Batch 1 结束时 PMTiles end-to-end 为 `NOT VERIFIED`。根因不在消费侧（`addProtocol` 接入、
HTTP Range 均已修好），而在**生产侧**：正式 `PmtilesWriter` 生成的归档不是合法 PMTiles v3。

## 2. 与原规范的逐项不一致

| # | 项目 | 修复前 | PMTiles v3 要求 | 影响 |
|---|---|---|---|---|
| 1 | root directory 编码 | JSON（`{"rootDir":{"tiles":[…]}}`） | varint 二进制：entry count / TileID 增量 / RunLength / Length / Offset | 任何合规 reader 在解析目录时失败 |
| 2 | TileID | `((1<<2z)-1)/3 + (y<<z) + x`（行主序） | Hilbert 曲线累积位置 | 目录查找全部错位 |
| 3 | internal_compression | `0`（Unknown） | `1`（None，目录与 metadata 未压缩） | 声明与实际内容不符 |
| 4 | tile_compression | `1`（None） | `2`（Gzip，每条 MVT 为独立 gzip 流） | 声明与实际内容不符 |
| 5 | metadata.type | `"vector"` | `baselayer` / `overlay` | 非规范取值 |
| 6 | addressed_tiles_count | `0` | 实际 tile 数 | 计数缺失 |
| 7 | leaf directory | 无实现 | 超 16384 字节须拆分，root 存 leaf 指针 | 大归档必然违规且无检测 |
| 8 | clustered | 固定 `0` | 与真实 layout 一致 | 丢失顺序信息 |
| 9 | bounds/center | 截断取整 | int32 E7 | 负值方向性偏差（次要） |

## 3. 附带发现并修复的两项缺陷（超出任务书列出的四项）

### 3.1 MVT Value 整数编码字段号错误（影响生产渲染）

`MvtEncoder.EncodeValue` 把整数写成 **field 2 + wire type 0**。`vector_tile.proto` 中
field 2 是 `float_value`，wire type 必须为 5；`int_value` 是 **field 4**。

后果：`TileBuilder` 给每条 road 要素 `speed_limit`（int）、给每个 junction 要素
`movements`（int），MapLibre 遇到字段号与线型不匹配时**静默丢弃该要素**——浏览器中
道路与交叉口完全不渲染，且无任何 error。

判别证据（同一归档、同一瓦片、仅改标签类型，真实 Chromium 渲染像素统计）：

| 标签类型 | road 像素 | junction 像素 | poi 像素 |
|---|---|---|---|
| 含数值标签（修复前行为） | 0 | 0 | 156 |
| 仅字符串标签 | 6960 | 84 | 156 |

修复为 field 4；并移除「未知类型写出零字节 Value」的静默行为，改为抛
`NotSupportedException`。

### 3.2 TileBuilder 道路几何退化为孤立点

`TileBuilder` 以「两个各含 2 个元素的段」传入道路两端点：
`Points = new[] { new[]{x0,y0}, new[]{x1,y1} }`。而 `MvtEncoder.EncodeGeometry` 把每个
段视为一串扁平 `x,y,x,y…` 坐标，故每段 `pointCount = 2/2 = 1`，只发出 `MoveTo` 而
从不发出 `LineTo`——道路被编码成两个单点 LineString，永远无法渲染。

修复为单段扁平序列 `new[]{x0,y0,x1,y1}`。

## 4. root / leaf directory 设计

- 条目按 tile_id 升序；数据按同序写入，故 `clustered = 1`（并对偏移单调性显式校验）。
- 全部条目可放入 root 时 leaf 段为空。
- 超出 `16384 - 127` 字节时按 2 的幂均分拆 leaf，保持一层；root 存 leaf 指针
  （TileID = leaf 首项 TileID，RunLength = 0，Offset/Length 指向 leaf 段）。
- root 仍超限且 leaf 数已达条目数时 **fail-fast 抛错**，不生成违规归档。
- 实测：11475 条目 → root 17 字节（2 个 leaf 指针），leaf 段 57463 字节，
  `127 + 17 = 144 <= 16384`。

## 5. 验证

### 5.1 C# 规范符合性测试（新增项目 `tests/ScsVectorTiles.Tests`）

29 项，读取侧为按规范文本独立实现的解码器（不复用 writer 代码）：

- T1 header 字段、偏移衔接、compression/tile_type/zoom/bounds/center
- T1 internal=None 与目录真实字节一致
- T2 TileID 官方 reference vectors（z0/z1/z2/z12，含 `z12 x3423 y1763 -> 19078479`）、
  同 zoom 双射、跨 zoom 首尾相接
- T3 root 首字节非 `{`、条目升序、run_length、counts 与目录一致、相对偏移编码
- T4 瓦片 gzip 往返字节级一致、未收录瓦片返回空
- T5 大目录拆分 leaf、root+header ≤ 16384、全量瓦片可读、leaf 指针有序且覆盖 leaf 段
- T6 MVT Value 字段号/线型（含负数 int64 varint、非法类型抛错）
- T7 MVT 几何段语义（两点线必须发出 LineTo；拆段退化为两 MoveTo 的反面守卫）
- metadata 为 JSON 且 type=baselayer

### 5.2 独立 reader 互操作（官方 `pmtiles` npm 5.2 序列，与前端同版本）

`tools/ets2nav-web/scripts/verify-pmtiles.mjs`，支持文件（FileSource）与 HTTP（FetchSource）：

| 归档 | 条目 | 结果 |
|---|---|---|
| 夹具（802 条） | 文件 + HTTP | header/metadata/counts 全 PASS，TileID 与官方 `zxyToTileId` 802/802 一致，802 块瓦片解压字节与原始 MVT 完全一致 |
| 夹具 `--large`（11475 条，含 leaf） | 文件 | root 17 字节、leaf 57463 字节；11475/11475 一致 |
| 夹具 `--minimal`（1374 B < 16384） | 文件 + HTTP | 触发「探测区间不可满足 → 416 → 客户端按 Content-Range 回退」路径并通过 |
| **真实生产归档**（map-inspector → TileBuilder → PmtilesWriter，Berlin 8 扇区） | 文件 | 6 块瓦片全部可读可解析，实际图层覆盖 metadata 声明的 road/junction/city/poi |

### 5.3 HTTP Range 实测矩阵（21 项 PASS）

200 全量 / 206 + Content-Range + Content-Length / Accept-Ranges / 端点越界截断 /
起点越界 416 + `bytes */len` + 无 body / HEAD 无 body（含 HEAD+Range）/
多区间忽略为 200 / PMTiles 首窗口 bytes=0-16383。206 响应体与磁盘对应区间做字节级比对。

### 5.4 真实 Chromium + MapLibre

| 数据源 | GET /map.pmtiles | map.loaded | 渲染要素 |
|---|---|---|---|
| 夹具（柏林调试坐标） | 206 × 15 | true | road 9 / junction 6 / poi 6 / vehicle 1 |
| **真实生产归档** | 206 × 3 | true | road 2205 / junction 693 / poi 79 / vehicle 1 |

console 无 PMTiles parse error、无 varint error；仅剩字体探测 404（预期，走 WARN 降级）。

### 5.5 门

- `dotnet test MapCompiler.sln`：9 个项目全绿，共 105 项（其中 ScsVectorTiles.Tests 29 项）
- `dotnet build tools/map-inspector/...`：0 错误
- `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test`：0 退出码
- `git diff --check`：0

## 6. Release 影响

`ets2nav-dataset-europe-v5.zip` 经检查**不含** `map.pmtiles`（仅 diagnostics.json /
junction.graph / manifest.json / map.db / README-dataset.txt / routing.graph / search.db）。
因此本次修复**不需要重发 routing dataset**；`map.pmtiles` 为本地生成物（`*.pmtiles` 受
`.gitignore` 约束），由 `map-inspector --tiles` 重新生成即可。

## 7. 已知限制与未处理项

1. `tools/ets2nav-web/map.pmtiles`（源码目录内）仍是早期手工放置的 JSON 桩，**不是合法
   PMTiles**。本轮未覆盖该文件（属未跟踪的运行期产物，避免破坏用户本地状态）。
   重新生成：`tools\map-inspector\MapInspector\bin\Debug\net9.0\map-inspector.exe --install <ETS2 安装目录> --sectors <扇区列表> --tiles tools\ets2nav-web\map.pmtiles`。
2. `TileBuilder` 仍不裁剪跨瓦片线段（线要素放入中点所在瓦片）——P1-12 已登记的调试级简化。
3. `TileBuilder.cs` 存在一个未使用的本地函数 `AddToTile`（编译器警告 CS8321），
   属既有死代码，本轮未清理以控制范围。
4. `tools/graph-debugger/index.html` 仍使用旧的 `type:'pmtiles'` 接入并从 unpkg 加载
   MapLibre，未修（按 §12 继续登记为 follow-up）。
5. city 文字层仍因缺本地 `fonts/` 而跳过（Batch 1 决策 3），非本轮回归。
