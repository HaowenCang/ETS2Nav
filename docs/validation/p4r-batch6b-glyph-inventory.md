# P4R Batch 6B —— City Glyph Coverage Inventory（2026-09-13）

本轮解决的问题是一个**判据层级错误**：RC 不得以「存在 `0-255.pbf`」推得「city 标签完整」。存在性只证明某一块字形分片被放置过，既不证明该分片来自预期字体，也不证明它覆盖了 city 图层实际要渲染的字符集。

判定用语沿用仓库既有三层：`PASS` 表示在本报告写明的地点与方式下真实执行且通过；`NOT VERIFIED` 表示未执行或无法执行；`PARTIAL` 表示部分成立且写明缺哪一部分。**没有把未执行写成 `PASS`。**

---

## 0. 结论表

| 项目 | 判定 | 依据 |
| --- | --- | --- |
| 字体栈字符串 | **PASS** | `tools/ets2nav-web/app.js:161`，值确为 `"Open Sans Regular"`；§1 |
| 仓库内是否存在字形（glyph/SDF）生成器 | **PASS（否定结论）** | `map-compiler/**` 对 `glyph\|font\|sdf\|pbf` 零命中；§2 |
| 字形分片由谁提供 | **PASS** | 外部提供，仓库只做透传与存在性记录；§2 |
| 全树是否存在 `*.pbf` / Open Sans 字体文件 | **PASS（否定结论）** | 递归枚举结果为空；§3 |
| `search.db` 实际格式 | **PASS** | 头部 16 字节逐字节比较命中 `SQLite format 3\0`；§4.1 |
| city 语料规模 | **PASS** | `type='City'` 1135 行 / 380 去重名，与 `manifest.stats.cities` 相等；§4.2 |
| city 语料码点清单 | **PASS** | 27 个 distinct 码点，全部落在 `U+005F`、`U+0061..U+007A`；§4.3 |
| 语料是否落在 `0-255` 内 | **PASS** | 27/27 落在 `U+0000..U+00FF` 内，越界 0 个；§5 |
| 渲染该语料所需字形分片集合 | **PASS** | `{ 0-255 }`，单分片；§5 |
| 标签文本是否为人类可读城市名 | **PARTIAL** | 380/380 匹配 `/^[a-z_]+$/`，即瓦片标签是**城市 token** 而非本地化名称；§6 |
| Open Sans Regular 自身是否覆盖该字符集 | **NOT VERIFIED** | 树内无任何字体二进制，本轮禁止下载；§7 |
| 真实欧洲 `map.pmtiles` 的 city 图层字符集 | **NOT VERIFIED** | `ETS2_INSTALL` / `ETS2NAV_EXTRACTED` 未设置，无法生成；§7 |
| 仓库内既有 `map.pmtiles` 能否替代 | **PASS（否定结论）** | 实测为 3 个城市的柏林 8 扇区样本，非欧洲全量；§8 |

---

## 1. 字体栈判定（引源）

`TILE_FONT_STACK` **确为 `"Open Sans Regular"`**，定义在 `tools/ets2nav-web/app.js:161`：

```js
const TILE_FONT_STACK = "Open Sans Regular";
```

它同时是 style 的 `glyphs` 模板与实际文字图层的 `text-font` 的唯一来源。`app.js:29-31` 给出模板，且附有离线约束注释：

```js
    // 离线约束（P4R-01）：glyphs 只能指向同源本地路径，不得指向运行时公共 CDN。
    // 本地字体缺失时仅跳过 city 文字层，分级处理见 loadTiles。
    glyphs: "vendor/fonts/{fontstack}/{range}.pbf",
```

`app.js:238-240` 把同一常量代入文字图层：

```js
    map.addLayer({ id: "tile-city", type: "symbol", source: "tiles", "source-layer": "city",
      layout: { "text-field": ["get", "name"], "text-size": 11, "text-font": [TILE_FONT_STACK] },
      paint: { "text-color": "#fff" } });
```

全树范围内该字体栈取值出现于 3 个来源文件，没有第二套候选值（以 `Open Sans Regular` / `Open%20Sans` 两种写法检索；另在 `docs/validation/p4r-batch6a-2026-09.md:293` 有一处文档引用）：

| 位置 | 形态 | 性质 |
| --- | --- | --- |
| `tools/ets2nav-web/app.js:161` | 常量 `TILE_FONT_STACK` | 正式前端，唯一权威来源 |
| `tools/ets2nav-web/tests/e2e/02-map.spec.mjs:78` | 字面量 `vendor/fonts/Open%20Sans%20Regular/0-255.pbf` | E2E 断言，硬编码同一取值（独立佐证目录名含空格） |
| `tools/graph-debugger/index.html:54` | 字面量 `'Open Sans Regular'` | 调试工具，另起一套：其 `glyphs` 指向 `https://demotiles.maplibre.org/font/{fontstack}/{range}.pbf`（同文件 `:36`），**不遵守** §1 所述的离线同源约束 |

`tools/graph-debugger/**` 属本轮禁止修改范围，此处仅登记差异，不作为本轮结论的一部分。

字体栈字符串本身**未携带任何版本信息**：仓库内不存在字体的版本、上游 URL、许可证或校验和记录（§8 的键可得性由此推定）。

---

## 2. 字形产线判定：仓库**不能**自行生成字形

结论是**否**，且该结论来自穷尽检索而非推断。

在项目自有代码范围内（`map-compiler/`、`tools/ets2nav-web/scripts|tests`、`scripts/`、`desktop/scripts`、`docs/`、`nav-core/`，排除 `node_modules/`、`obj/`、`bin/`、`dist/`、`target/`）以 `glyph`、`sdf`、`fontnik`、`.pbf` 四个模式检索，命中项**全部是消费侧或文档侧**，无一为生成侧：

| 位置 | 命中内容 | 性质 |
| --- | --- | --- |
| `tools/ets2nav-web/tests/e2e/02-map.spec.mjs:76,78` | 注释 + `0-255.pbf` HEAD 探测 | 消费侧断言 |
| `tools/ets2nav-web/tests/e2e/helpers.mjs:3,11,13,15,39,40` | `GLYPH_URL_MARKER = "/vendor/fonts/"` 白名单 | 诊断过滤 |
| `desktop/scripts/assemble-bundle.ps1:21,25,54,173,174,187,189` | `-FontsDir` 透传与记录 | 复制，不生成 |
| `desktop/scripts/verify-bundle.ps1:108` | 打印小节标题 | 校验，不生成 |

`map-compiler/` 对上述四个模式的命中数为 **0**，即矢量瓦片编译器**完全不涉及字形**——这与其职责边界一致：`map-compiler/src/ScsVectorTiles/**` 只产出瓦片的几何与属性。

前端构建同样只做透传。`tools/ets2nav-web/scripts/build.mjs:229-235`：

```js
  const fontsSrc = join(ROOT, "fonts");
  if (existsSync(fontsSrc)) {
    await cp(fontsSrc, join(DIST_VENDOR, "fonts"), { recursive: true });
    optional.push("fonts/");
  } else {
    console.warn("[build] 提示：未提供 fonts/——运行期跳过 city 文字层（几何图层照常）");
  }
```

打包侧的唯一门是**存在性 + 地点正确**。`desktop/scripts/assemble-bundle.ps1:173-176` 的注释明确了目录约定的脆弱性：

```powershell
    # 目标路径必须与前端 style 的 glyphs 模板一致：app.js 用
    # `vendor/fonts/{fontstack}/{range}.pbf`，build.mjs 也把 <web>/fonts 复制到
    # dist/vendor/fonts。放错目录不会报错，只会让文字层静默不出现——因此这里按
    # 服务端实际提供的路径写入，并由 verify-bundle.ps1 用同一路径核对。
```

因此：**`{range}.pbf` 必须由外部提供**。仓库对其唯一的要求是「放在 `web/vendor/fonts/` 下」以及「提供一份 provenance」。本轮环境不具备提供它们的条件（§7）。

---

## 3. 全树字体与字形文件枚举（实测为空）

对仓库根目录递归枚举以下类别，结果如下（含 `node_modules/`、`vendor/`、构建产物目录）：

| 检索条件 | 结果 |
| --- | --- |
| `*.pbf`（任意位置） | **0 个文件** |
| 文件名匹配 `Open.?Sans` | **0 个文件** |
| `*.ttf` / `*.otf` / `*.woff` / `*.woff2` | 23 个，**全部与本项目字体栈无关** |

那 23 个字体文件分布在两处**未跟踪的第三方参考克隆**中，均非 `Open Sans`，也均不在构建路径上：

- `vendor/ref/ets2la/**/Assets/Fonts/Geist-*.ttf`（18 个，Geist 字族，`Assets/Fonts` 与 `ETS2LA.UI/Assets/Fonts` 各 9 个）
- `vendor/ref/easy-scsmodmanager/**/resources/fonts/InterVariable.ttf`、`NotoColorEmoji.ttf`（2 个）
- `tools/ets2nav-web/node_modules/playwright-core/**/codicon-*.ttf`（3 个，Playwright 自带图标字体）

`vendor/ref/` 不受版本控制（`git ls-files vendor/ref` 为空），`node_modules/` 亦不跟踪。`*.pbf` 与 Open Sans 的枚举为空，**证实「字体文件不在仓库内」这一主张**，且不存在任何测试夹具字形文件可供替代。

---

## 4. 城市名语料与码点清单

### 4.1 `search.db` 的实际格式

不假设格式，直接读文件头 16 字节与魔数逐字节比较。实测：

```
头部 16 字节    53 51 4c 69 74 65 20 66 6f 72 6d 61 74 20 33 00
头部 ASCII      "SQLite format 3\u0000"
SQLite 魔数     匹配
```

该 16 字节序列即 `SQLite format 3\0`，因此 `search.db` **确实是 SQLite**，与 `data/europe-v5/manifest.json:1141-1143` 声明的 `"format": "sqlite-fts5"` 一致。表结构实测为：

```
poi, poi_fts, poi_fts_config, poi_fts_data, poi_fts_docsize, poi_fts_idx
CREATE TABLE poi (id INTEGER PRIMARY KEY, type TEXT, name TEXT, x REAL, z REAL, access_node TEXT, meta TEXT)
```

（`map.db` 为同类 SQLite，含 `roads`/`junctions`/`movements` 三表，**不含城市名**。）

### 4.2 语料的推导链与规模

城市名在数据集内的落点由代码确定，而非由命名猜测：

1. 瓦片 `city` 图层的 `name` 取自 `CityItem.City`——`map-compiler/src/ScsVectorTiles/TileBuilder.cs:93-101`：
   ```csharp
   var cityNodes = sectors.SelectMany(s => s.Items.OfType<CityItem>())
       .GroupBy(c => c.City).Select(g => g.First()).ToList();
   foreach (var c in cityNodes)
   ...
       AddPoint(tileFeatures, tx, ty, ..., "city", new Dictionary<string, object> { ["name"] = c.City });
   ```
2. 同一 token 亦是 `search.db` 的 City POI 名——`map-compiler/src/ScsMapModel/PoiExtractor.cs:41-42`：
   ```csharp
   foreach (var c in sec.Items.OfType<CityItem>())
       Add(result, PoiType.City, c.City, c.NodeUid, nodePos, null);
   ```

因此 `poi` 表中 `type='City'` 的 `name` 集合是**瓦片 city 图层标签语料的超集**（瓦片侧另有 `GroupBy` 去重与 `nodeIndex` 命中过滤，只会更少）。实测规模：

| 量 | 实测值 |
| --- | --- |
| City 行数 | 1135 |
| City 去重名数 | 380 |
| `manifest.stats.cities` | 1135（与行数相等，交叉核对通过） |
| 全部 POI 行数 / 去重名数 | 7918 / 669 |

1135 与 manifest 声明**相等**，说明该语料与数据集统计自洽，不存在漏抽。

### 4.3 码点清单（实测）

对 1135 行逐字符统计（同一行内重复字符计入多次）：

| 量 | 实测值 |
| --- | --- |
| distinct 码点数 | **27** |
| 码点范围 | `U+005F` .. `U+007A` |
| 非 ASCII（> `U+007F`） | 0 |
| 超出 `U+00FF` | 0 |
| 字符总出现次数 | 8089 |

按 Unicode 区块聚合只有一个区块，因此「按频次排序的区块表」在该语料上退化为单行：

| 区块 | 范围 | 码点数 | 出现次数 |
| --- | --- | --- | --- |
| Basic Latin | `U+005F..U+007A` | 27 | 8089 |

把 Basic Latin 细分为子区后才有分辨力，实测：

| 子区 | 码点数 | 出现次数 |
| --- | --- | --- |
| 小写 `a-z` | 26 | 8038 |
| 标点（`[\]^_\``）其中仅 `_` | 1 | 51 |
| 数字 `0-9` | 0 | 0 |
| 大写 `A-Z` | 0 | 0 |
| 空格 | 0 | 0 |
| 其它 ASCII 标点 | 0 | 0 |

即完整字符集为 `_` 与 `a`–`z`，**共 27 个码点**，不含数字、大写、空格或任何重音字符。出现次数最高的 12 个码点：

| 码点 | 字符 | 出现次数 | 所属去重名数 |
| --- | --- | --- | --- |
| `U+0061` | `a` | 1108 | 243 |
| `U+0072` | `r` | 673 | 202 |
| `U+0065` | `e` | 671 | 175 |
| `U+0069` | `i` | 638 | 167 |
| `U+006F` | `o` | 586 | 162 |
| `U+006E` | `n` | 579 | 157 |
| `U+006C` | `l` | 533 | 153 |
| `U+0073` | `s` | 477 | 148 |
| `U+0074` | `t` | 377 | 102 |
| `U+0075` | `u` | 277 | 87 |
| `U+0067` | `g` | 255 | 71 |
| `U+0062` | `b` | 228 | 75 |

辅助口径（`poi` 表全部 `type` 的 `name`，含 company / fuel / garage 等非城市名）为 31 个 distinct 码点，**超出 `U+00FF` 的同样为 0 个**。引入该口径是为了降低「城市名落在别处」的风险，其结论与主口径一致。

---

## 5. 对 `0-255` 的覆盖判定

判定基准取 MapLibre/Mapbox 字形协议的分片单位：每个 `{range}.pbf` 覆盖 256 个连续码点，文件名即 `<下界>-<上界>`。

实测结果：

- 落在 `U+0000..U+00FF` 内的码点：**27 / 27**；
- 落在该范围外的码点：**0 / 27**；
- 因语料最高码点为 `U+007A`，实际请求的字形分片集合为 **`{ 0-255 }`**，单分片即可覆盖；全部 POI 语料同样只需 `0-255`。

**结论：实测语料完整落在 `0-255` 范围内，且所需字形分片集合恰为 `{0-255}` 一个。**

该结论的支撑量是 §4.3 的 27 个码点（`U+005F`、`U+0061..U+007A`）与 8089 次出现，而不是「`0-255.pbf` 文件存在」。反过来，本节结论**不能**推广为「city 标签完整」：它只说明**在标签文本等于城市 token 的前提下**，字形侧不构成显示障碍。标签文本本身的问题见 §6，字体侧的确证见 §7。

需要与本节结论一并记录的是渲染路径的存在性门：`app.js:226-236` 只探测 `0-255.pbf`，其通过与否决定整个 `tile-city` 图层是否创建；而按 Mapbox/MapLibre 字形协议，运行期分片是按 `text-font` 逐 256 码点区间向 `glyphs` 模板惰性请求的（这一点由模板中的 `{range}` 占位与分片命名约定推得，本轮因无真实字体与瓦片而**未实测**）。由于实测所需分片集合只有一个，该存在性门在本语料下与「覆盖完整」等价——但这一等价**依赖于语料不变**，一旦标签文本改为本地化名称（§6），等价即失效，而现有校验不会察觉（§9）。

---

## 6. 标签文本形态：token 而非人类可读名称

`TileBuilder.cs:101` 使用 `c.City`，即从扇区文件读出的**城市 token**（`SectorFile.cs:421-430` 的 `r.ReadToken()`）。人类可读名称存在于另一条路径：`DefinitionResolver.cs:149` 从 `/def/city/*.sui` 读取 `city_name_localized`：

```csharp
DisplayName = Str(u, "city_name_localized") ?? Str(u, "city_name"),
```

该字段**不进入任何数据集产物**，`TileBuilder` 与 `PoiExtractor` 均未引用它。实测该 token 语料 380/380 匹配 `/^[a-z_]+$/`，形如 `wroclaw`、`zurich`、`a_coruna`、`novi_sad`、`targu_mures`。

因此本轮可确证的是一个**双向受限**的结论：字形侧不构成障碍（§5），但 city 图层实际渲染的文本是内部 token，不含任何重音字符或大写。这解释了为何一个「需要多语言字形覆盖」的直觉问题，在本数据集上的实测答案反而是单分片即可。同时它意味着一项**与字形无关**的既有限制：以本地化名称渲染城市标签的能力，当前管线并不具备，且该能力一旦引入，字形需求会立即越出 `0-255`。

---

## 7. `NOT VERIFIED` 清单

以下各项**未执行**，也未以任何替代物顶替。原因是同一组：本轮环境无法获得真实欧洲 `map.pmtiles`（`ETS2_INSTALL` 与 `ETS2NAV_EXTRACTED` 在 process / user / machine 三个作用域均未设置，且本轮禁止定位或猜测用户的游戏安装目录），且不允许下载字体。

1. **真实欧洲 `map.pmtiles` 的 city 图层字符集**——未生成、未读取。需要真实数据集与 `map-compiler` 的 `--tiles` 路径。§4 的语料是该图层的**超集推导**，不是其直接测量。
2. **欧洲全量构建下 city 图层的实际标签条数**——需要真实 `map.pmtiles`。注意 `TileBuilder` 会丢弃 `NodeUid` 不在 `nodeIndex` 中的城市，故实际条数 ≤ 380 去重名。
3. **`Open Sans Regular` 是否覆盖 `U+005F`、`U+0061..U+007A`**——树内无字体二进制（§3 实测），本轮禁止下载，故无法验证。此处仅记录推理边界：所需字符全部属 ASCII 基本区，主流 Open Sans 发行版均覆盖之，但**该判断来自上游常识而非本轮测量**，故不记为 `PASS`。
4. **`Open Sans` 的版本、上游来源与许可证**——仓库未固定任何版本（§1）。需注意上游存在两条谱系（2011 年静态字重版与 2021 年后的可变字体重设计版），二者的码位覆盖与度量不同；未取得实际字体前无法确定属于哪一支。因此 §10 提案中 `version`、`upstream`、`license_spdx`、`source_sha256` 四键在**当前环境均不可得**。
5. **字形分片是否真由该字体生成**——需要实际 `{range}.pbf` 与其生成命令。现有校验只记录存在性、文件数、字节数与树摘要，**不校验来源**（§9）。
6. **运行期 `tile-city` 图层在真实字体下的渲染结果**——需要真实 `map.pmtiles` **与**真实字形分片同时具备。现有 E2E（`02-map.spec.mjs:68-83`）在两者任一缺失时走降级分支，只断言几何图层不受影响，不对文字渲染作断言。
7. **多语言标签所需的码点范围**——若引入本地化名称（§6），所需范围需按本地化语料重新测量；该语料不在数据集内，不可得。

**已执行且为否定结论、因而不属于 `NOT VERIFIED` 的项目**：仓库无字形生成器（§2）；全树无 `*.pbf`、无 Open Sans 字体文件（§3）；`search.db` 格式判定（§4.1）。

---

## 8. 既有 `map.pmtiles` 不可作为本轮证据

全树 `*.pmtiles` 共 6 个文件，其中 4 个位于 `desktop/target/**`（Tauri 构建把 web 资源编入的副本），源码侧实存 2 个：`tools/ets2nav-web/map.pmtiles` 与 `tools/ets2nav-web/dist/map.pmtiles`，二者 114885 B、mtime 均为 2026-08-11 23:34:59。两者均受 `.gitignore` 约束（`.gitignore:41: *.pmtiles`、`.gitignore:6: dist/`），属未跟踪的运行期产物。

对其结构的实测：

| 项 | 实测值 |
| --- | --- |
| 魔数 / spec 版本 | `PMTiles` / 3 |
| 文件长度 | 114885 B |
| 瓦片条目数 / 内容数 | 6 / 6 |
| `numAddressedTiles` | 0 |
| minZoom / maxZoom | 6 / 11 |
| 经度范围 / 纬度范围 | 0.0709..0.1446 / −0.1441..−0.0002 |
| offset 127 起 400 字节 | `{"rootDir":{"tiles":[{"tile_id":3445,...` |

offset 127 正是 PMTiles v3 头部长度，也是 root directory 的起始位置；该处内容是 **JSON 文本**而非二进制 varint 索引。官方 `pmtiles@4.5.0` 读取该文件时在 `deserializeIndex` 抛 `Expected varint not more than 10 bytes`。该缺陷在本仓库内**已被登记**：`docs/validation/p4r-pmtiles-conformance-2026-09.md:129-131` 记其为「早期手工放置的 JSON 桩，不是合法 PMTiles」，`map-compiler/tests/ScsVectorTiles.Tests/PmtilesConformanceTests.cs:165` 的 `T3_RootDirectory_IsBinaryVarintNotJson` 即为此回归守卫。本文不重复该议题。

与本轮直接相关的是其**瓦片载荷**：绕过目录、直接解压 `tile_data` 段可得 6 块可解析 MVT，其 `city` 图层的 distinct 名称为 **3 个**——`berlin`、`dresden`、`szczecin`，与 `docs/validation/p4r-pmtiles-conformance-2026-09.md:96,109` 记载的「Berlin 8 扇区真实生产归档（6 块瓦片）」一致。

**结论：任何以该文件为依据的「city 标签完整」主张都不成立**，其城市数为 3，而数据集内去重城市名为 380、City 行为 1135。本轮未使用该文件替代欧洲地图，也未据此推断任何 §5 结论。

---

## 9. 现有校验对「覆盖」的缺口

与 §5 的等价关系直接相关的一项缺口：现有字体校验**不把 `ranges` 与磁盘上的分片做交叉核对**。

`desktop/scripts/assemble-bundle.ps1:330-333` 要求 provenance 含九个键，其中 `ranges` 被要求为「非空字符串」：

```powershell
    $prov = Read-Provenance -Path $FontsProvenance -Context 'fonts' -RequiredKeys @(
        'font', 'version', 'upstream', 'license_spdx', 'source_sha256',
        'generator', 'generator_version', 'generation_command', 'ranges'
    )
```

`source_sha256` 另有 64 位十六进制格式检查（`:334-336`），但 `ranges` 无格式约束、也无与 `vendor/fonts/` 实际内容的一致性检查。落到 bundle 清单时（`:442-448`）记录的是**磁盘测量值**而非来源声明：

```powershell
    fonts        = [ordered]@{
        dir         = 'web/vendor/fonts'
        present     = $fontsPresent
        files       = $fontsFiles
        bytes       = $fontsBytes
        tree_sha256 = $fontsDigest
        provenance  = $fontsProvenance
    }
```

`desktop/scripts/verify-bundle.ps1:118-126` 复核的是 presence 标志、文件数与字节数。因此当前形态下：声明 `ranges: "0-255"` 而实际放置任意一个同名文件可以通过全部检查。这正是本轮要消除的判据层级错误在**打包层**的残留形态。本轮只登记，不在禁止修改范围内实施修复。

---

## 10. RC 清单应携带的字体来源记录（提案，未实施）

以下 JSON 为提案形态。它与 §9 所述既有实现的差异只有一处：新增的三个键**已在 bundle 清单中存在**（`fonts.files` / `fonts.bytes` / `fonts.tree_sha256`，由磁盘测量而非由 provenance 文件声明），故提案实际是把既有测量值并入同一记录，并使其可被交叉核对。

```json
{
  "font": "Open Sans Regular",
  "version": null,
  "upstream": null,
  "license_spdx": null,
  "source_sha256": null,
  "generator": null,
  "generator_version": null,
  "generation_command": null,
  "ranges": ["0-255"],
  "files": 1,
  "bytes": null,
  "tree_sha256": null
}
```

逐键的当前可得性：

| 键 | 类型 | 当前可否取得 | 说明 |
| --- | --- | --- | --- |
| `font` | string | **可得** | `"Open Sans Regular"`，取自 `app.js:161`；与 `glyphs` 模板的 `{fontstack}` 段必须逐字相同（含空格） |
| `version` | string | **不可得** | 仓库未固定版本；上游存在静态版与可变字体重设计版两条谱系，未取得字体前无法判定（§7 第 4 项） |
| `upstream` | string | **不可得** | 需实际获取渠道（发行方与版本化 URL），当前无 |
| `license_spdx` | string | **不可得** | 需随字体取得许可证文本后确定；不得据字族名推定 |
| `source_sha256` | string(64 hex) | **不可得** | 需字体源文件；`assemble-bundle.ps1:334` 已强制其格式 |
| `generator` | string | **不可得** | 字形生成工具（仓库内不存在，§2）；需在生成侧记录 |
| `generator_version` | string | **不可得** | 同上 |
| `generation_command` | string | **不可得** | 同上；须为可复现的确切命令，且不得含绝对路径 |
| `ranges` | string[] | **可得（部分）** | 需求侧已实测为 `["0-255"]`（§5）；供给侧实际分片清单需字体到位后才能填 |
| `files` | integer | **不可得** | 随供给分片数而定；`assemble-bundle.ps1:349` 已在测量 |
| `bytes` | integer | **不可得** | 随供给分片而定；`:350` 已在测量 |
| `tree_sha256` | string(64 hex) | **不可得** | 随供给分片而定；`:351` 已在测量（`Get-TreeDigest`） |

九键中当前仅 `font` 完全可得、`ranges` 的需求侧可得；其余十项均需真实的字体源文件与字形生成步骤。提案的判定规则应当是：**以 §5 实测的 `ranges` 需求集为基准，要求 `files` 所覆盖的分片集合 ⊇ 该需求集**，而不是检查某个文件是否存在——这才是「city 标签字形完整」可被证伪的判据。

在真实字体与真实 `map.pmtiles` 均不可得的本轮，该记录无法据实填写，故不实施。

---

## 11. 复现方式

新增脚本 `scripts/analyze-city-charset.mjs`（Node.js，纯只读，脚本自身位置解析仓库根，不含机器相关绝对路径）。

```powershell
node scripts/analyze-city-charset.mjs
node scripts/analyze-city-charset.mjs --json <输出路径>
node scripts/analyze-city-charset.mjs --all-codepoints --top 20
```

退出码语义：`0` 成功；`1` FAIL（数据存在但与契约不符）；`3` PRECONDITION（数据缺失，无法测量）。

实测退出码（逐条真实执行）：

| 场景 | 命令要点 | 退出码 |
| --- | --- | --- |
| 正常测量 | 缺省数据集 | `0` |
| 数据集目录不存在 | `--dataset <不存在目录>` | `3` |
| `search.db` 头部非 SQLite | 临时目录内写入非 SQLite 同名文件 | `1` |
| 是合法 SQLite 但无 `poi` 表 | 临时目录内建仅含 `other` 表的库 | `3` |
| 未知参数 | `--bogus` | `1` |

上一次运行的实测指纹：

```
script sha256     c0b4f5af4ccc24bfef8763c9e5fd2706cb1106b9de2f699c2eb1a887c3481409
search.db sha256  8e61f766702994056bac21fcdd16ff844e5df43218914c272ea822f662900bc0
search.db bytes   671744
node              v24.13.0
```

脚本经由 `node:sqlite`（Node ≥ 22.5）读取，并在加载该模块前摘除默认 warning 监听，以免 `ExperimentalWarning` 污染证据输出。它不读取任何游戏原始档案，仅使用工作树内已存在的 `data/europe-v5/search.db` 与 `manifest.json`；该目录受 `.gitignore:52: data/` 约束，故脚本对数据缺失以退出码 `3` 显式区分「无法测量」与「测量失败」。
