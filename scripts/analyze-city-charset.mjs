#!/usr/bin/env node
// ETS2Nav P4R Batch 6B — city 图层字形覆盖度清单（city glyph coverage inventory）
//
// 目的：为「city 文字层的标签字符集是否落在 0-255 字形范围内」这一 RC 断言提供**可复算的
// 实测量**，而不是以「存在 0-255.pbf」作为结论。
//
// 数据来源与推导链（全部来自仓库内已存在的产物，不读取任何游戏原始档案）：
//   1. 瓦片 city 图层的 name 属性来自 CityItem.City（城市 token）——
//      map-compiler/src/ScsVectorTiles/TileBuilder.cs:93-101：
//        sectors.SelectMany(s => s.Items.OfType<CityItem>()).GroupBy(c => c.City) ...
//        AddPoint(..., "city", new Dictionary<string, object> { ["name"] = c.City });
//   2. 同一 token 也是 search.db 的 City POI 名——
//      map-compiler/src/ScsMapModel/PoiExtractor.cs:41-42：
//        foreach (var c in sec.Items.OfType<CityItem>()) Add(result, PoiType.City, c.City, ...);
//   3. 因此 search.db（SQLite FTS5，ADR-004）中 type='City' 的 name 集合既是数据集内
//      city 名称的唯一权威来源，也是 map.pmtiles 的 city 图层标签语料的**超集**
//      （瓦片侧另有 GroupBy 去重与 nodeIndex 命中过滤，只会变少不会变多）。
//
// 本脚本不假设 search.db 是 SQLite：它先读文件头 16 字节并与魔数逐字节比较，把比较
// 结果作为结论的一部分输出；仅在魔数成立时才按 SQLite 打开。
//
// 退出码（与仓库既有语义一致）：
//   0  清单生成成功
//   1  FAIL —— 数据存在但不符合既定契约（例如文件头不是 SQLite、行数与 manifest 统计不符）
//   3  PRECONDITION —— 数据缺失（数据集目录或 search.db 不存在、poi 表不存在），无法测量
//
// 用法：
//   node scripts/analyze-city-charset.mjs [--dataset <目录>] [--json <输出路径>]
//                                        [--all-codepoints] [--top <N>]
// 缺省数据集目录为 <仓库根>/data/europe-v5，按脚本自身位置解析，不含任何机器相关绝对路径。

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_DATASET = join(ROOT, "data", "europe-v5");
const SQLITE_MAGIC = "SQLite format 3\u0000";

const EXIT_OK = 0;
const EXIT_FAIL = 1;
const EXIT_PRECONDITION = 3;

// node:sqlite 在 Node 22.5+ 提供，但会打印 ExperimentalWarning。该警告对判定无意义，
// 且会污染 stdout/stderr 证据，故在加载模块前摘除默认 warning 监听（动态 import 保证顺序）。
process.removeAllListeners("warning");
process.on("warning", () => {});

function fail(msg, code = EXIT_FAIL) {
  console.error(`[charset] ${code === EXIT_PRECONDITION ? "PRECONDITION" : "FAIL"}：${msg}`);
  process.exit(code);
}

function parseArgs(argv) {
  const out = { dataset: DEFAULT_DATASET, json: null, allCodepoints: false, top: 12 };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--dataset") out.dataset = resolve(argv[++i] ?? "");
    else if (a === "--json") out.json = resolve(argv[++i] ?? "");
    else if (a === "--all-codepoints") out.allCodepoints = true;
    else if (a === "--top") out.top = Number.parseInt(argv[++i], 10);
    else fail(`未知参数 ${a}`);
  }
  return out;
}

/**
 * Unicode 区块表。仅用于把实测码点归类到可读的区块名；
 * 未列入的码点归入 "其他（未列区块）"。区块边界取 Unicode 15.1 的区块名与端点。
 */
const BLOCKS = [
  [0x0000, 0x007f, "Basic Latin"],
  [0x0080, 0x00ff, "Latin-1 Supplement"],
  [0x0100, 0x017f, "Latin Extended-A"],
  [0x0180, 0x024f, "Latin Extended-B"],
  [0x0250, 0x02af, "IPA Extensions"],
  [0x02b0, 0x02ff, "Spacing Modifier Letters"],
  [0x0300, 0x036f, "Combining Diacritical Marks"],
  [0x0370, 0x03ff, "Greek and Coptic"],
  [0x0400, 0x04ff, "Cyrillic"],
  [0x0500, 0x052f, "Cyrillic Supplement"],
  [0x0530, 0x058f, "Armenian"],
  [0x0590, 0x05ff, "Hebrew"],
  [0x0600, 0x06ff, "Arabic"],
  [0x0900, 0x097f, "Devanagari"],
  [0x0e00, 0x0e7f, "Thai"],
  [0x1e00, 0x1eff, "Latin Extended Additional"],
  [0x1f00, 0x1fff, "Greek Extended"],
  [0x2000, 0x206f, "General Punctuation"],
  [0x2070, 0x209f, "Superscripts and Subscripts"],
  [0x20a0, 0x20cf, "Currency Symbols"],
  [0x2100, 0x214f, "Letterlike Symbols"],
  [0x2150, 0x218f, "Number Forms"],
  [0x2190, 0x21ff, "Arrows"],
  [0x2200, 0x22ff, "Mathematical Operators"],
  [0x2460, 0x24ff, "Enclosed Alphanumerics"],
  [0x2500, 0x257f, "Box Drawing"],
  [0x25a0, 0x25ff, "Geometric Shapes"],
  [0x2600, 0x26ff, "Miscellaneous Symbols"],
  [0x2c60, 0x2c7f, "Latin Extended-C"],
  [0xa720, 0xa7ff, "Latin Extended-D"],
  [0xfb00, 0xfb4f, "Alphabetic Presentation Forms"],
  [0xfe50, 0xfe6f, "Small Form Variants"],
  [0xff00, 0xffef, "Halfwidth and Fullwidth Forms"],
];

function blockOf(cp) {
  for (const [lo, hi, name] of BLOCKS) if (cp >= lo && cp <= hi) return name;
  return "其他（未列区块）";
}

/** MapLibre/Mapbox 字形 PBF 的分片单位：每个 {range}.pbf 覆盖 256 个连续码点。 */
function rangeLabel(cp) {
  const lo = cp - (cp % 256);
  return `${lo}-${lo + 255}`;
}

/** Basic Latin 内的子区划分：用于说明语料实际用了哪几类字符（而不是「都在 0-255 内」一句带过）。 */
const ASCII_SUBRANGES = [
  [0x00, 0x1f, "C0 控制符"],
  [0x20, 0x20, "空格"],
  [0x21, 0x2f, "标点（!\"#$%&'()*+,-./）"],
  [0x30, 0x39, "数字 0-9"],
  [0x3a, 0x40, "标点（:;<=>?@）"],
  [0x41, 0x5a, "大写 A-Z"],
  [0x5b, 0x60, "标点（[\\]^_`）"],
  [0x61, 0x7a, "小写 a-z"],
  [0x7b, 0x7e, "标点（{|}~）"],
  [0x7f, 0x7f, "DEL"],
];

function subrangeOf(cp) {
  for (const [lo, hi, name] of ASCII_SUBRANGES) if (cp >= lo && cp <= hi) return name;
  return null;
}

function fmtCp(cp) {
  return `U+${cp.toString(16).toUpperCase().padStart(4, "0")}`;
}

/** 控制字符按字面量打印会破坏终端，用 Unicode 名称式占位。 */
function fmtChar(cp) {
  if (cp < 0x20 || cp === 0x7f) return `<control>`;
  if (cp === 0x20) return "<space>";
  return String.fromCodePoint(cp);
}

function main() {
  const args = parseArgs(process.argv.slice(2));

  let sqlite;
  try {
    sqlite = createRequire(import.meta.url)("node:sqlite");
  } catch (e) {
    fail(`本 Node 运行时不提供 node:sqlite（需要 Node >= 22.5）：${e.message}`);
  }

  // ── 前置条件：数据集与 search.db 必须存在
  const dbPath = join(args.dataset, "search.db");
  let dbBytes;
  try {
    dbBytes = readFileSync(dbPath);
  } catch (e) {
    fail(`数据集文件不存在或不可读：${dbPath}（${e.code ?? e.message}）`, EXIT_PRECONDITION);
  }
  const header = dbBytes.subarray(0, 16);

  // ── 格式判定：不假设 SQLite，逐字节比较魔数
  const magicAscii = header.toString("latin1");
  const isSqlite = magicAscii === SQLITE_MAGIC;
  const headerHex = Buffer.from(header).toString("hex").replace(/(..)(?=.)/g, "$1 ");
  console.log("== search.db 格式判定 ==");
  console.log(`  路径            ${dbPath}`);
  console.log(`  头部 16 字节    ${headerHex}`);
  console.log(`  头部 ASCII      ${JSON.stringify(header.toString("ascii"))}`);
  console.log(`  SQLite 魔数     ${isSqlite ? "匹配" : "不匹配"}`);
  if (!isSqlite) {
    fail(`search.db 头部不是 "SQLite format 3\\0"——文件格式与 PoiExtractor 契约不符，无法测量`);
  }

  const db = new sqlite.DatabaseSync(dbPath, { readOnly: true });

  // ── 前置条件：poi 表存在（ADR-004 / tools/map-inspector/MapInspector/Program.cs:695）
  const tables = db
    .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
    .all()
    .map((r) => r.name);
  console.log(`  表清单          ${tables.join(", ")}`);
  if (!tables.includes("poi")) {
    db.close();
    fail(`search.db 不含 poi 表——没有可用的城市名语料`, EXIT_PRECONDITION);
  }

  const schema = db
    .prepare("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'poi'")
    .get();
  console.log(`  poi schema      ${String(schema?.sql ?? "").replace(/\s+/g, " ")}`);

  // ── 语料：type='City' 的 name（主口径）
  const cityRows = db.prepare("SELECT name FROM poi WHERE type = 'City'").all();
  const allRows = db.prepare("SELECT type, name FROM poi").all();
  db.close();

  if (cityRows.length === 0) {
    fail("poi 表中没有 type='City' 的行——城市名语料为空", EXIT_PRECONDITION);
  }

  // ── 交叉核对 manifest.stats.cities（manifest 由 DatasetWriter 写出，cities = map.Cities.Count）
  let manifestCities = null;
  let manifestNote = "manifest.json 不可读，跳过交叉核对";
  try {
    const mf = JSON.parse(readFileSync(join(args.dataset, "manifest.json"), "utf8"));
    manifestCities = mf?.stats?.cities ?? null;
    manifestNote = manifestCities === null ? "manifest.stats.cities 缺失" : "已核对";
  } catch (e) {
    manifestNote = `manifest.json 读取失败：${e.code ?? e.message}`;
  }

  const distinct = [...new Set(cityRows.map((r) => r.name))].sort();
  const allDistinct = [...new Set(allRows.map((r) => r.name))].sort();

  console.log("");
  console.log("== 语料规模 ==");
  console.log(`  City 行数                     ${cityRows.length}`);
  console.log(`  City 去重名数                 ${distinct.length}`);
  console.log(`  manifest.stats.cities         ${manifestCities ?? "（不可得）"}   [${manifestNote}]`);
  console.log(`  全部 POI 行数                 ${allRows.length}`);
  console.log(`  全部 POI 去重名数             ${allDistinct.length}`);

  if (manifestCities !== null && manifestCities !== cityRows.length) {
    fail(
      `City 行数 ${cityRows.length} 与 manifest.stats.cities ${manifestCities} 不一致——` +
        `数据集统计与 search.db 内容矛盾`,
    );
  }

  // ── 码点清单（两个频次口径：全行出现次数 / 所属去重名数）
  function inventory(names, rowNames) {
    /** @type {Map<number, {occ:number, names:Set<string>}>} */
    const m = new Map();
    for (const n of rowNames) {
      for (const ch of n) {
        const cp = ch.codePointAt(0);
        let e = m.get(cp);
        if (!e) m.set(cp, (e = { occ: 0, names: new Set() }));
        e.occ++;
      }
    }
    for (const n of names) {
      for (const ch of new Set(n)) {
        const cp = ch.codePointAt(0);
        const e = m.get(cp);
        if (e) e.names.add(n);
      }
    }
    return m;
  }

  const inv = inventory(distinct, cityRows.map((r) => r.name));
  const invAll = inventory(allDistinct, allRows.map((r) => r.name));

  const cps = [...inv.keys()].sort((a, b) => a - b);
  const maxCp = cps[cps.length - 1];

  console.log("");
  console.log("== 码点清单（主口径：type='City' 全部行） ==");
  console.log(`  distinct 码点数               ${cps.length}`);
  console.log(`  码点范围                      ${fmtCp(cps[0])} .. ${fmtCp(maxCp)}`);
  console.log(`  非 ASCII（> U+007F）码点数    ${cps.filter((c) => c > 0x7f).length}`);
  console.log(`  超出 U+00FF 的码点数          ${cps.filter((c) => c > 0xff).length}`);

  // ── 按区块聚合，按「全行出现次数」降序
  const byBlock = new Map();
  for (const cp of cps) {
    const b = blockOf(cp);
    let e = byBlock.get(b);
    if (!e) byBlock.set(b, (e = { cps: 0, occ: 0, names: 0, lo: cp, hi: cp }));
    e.cps++;
    e.occ += inv.get(cp).occ;
    e.names = Math.max(e.names, inv.get(cp).names.size);
    e.lo = Math.min(e.lo, cp);
    e.hi = Math.max(e.hi, cp);
  }
  const blockRows = [...byBlock.entries()].sort((a, b) => b[1].occ - a[1].occ);

  console.log("");
  console.log("== 按 Unicode 区块（按全行出现次数降序） ==");
  console.log("  区块                                   范围                码点   出现次数");
  for (const [name, e] of blockRows) {
    console.log(
      `  ${name.padEnd(36)} ${(fmtCp(e.lo) + ".." + fmtCp(e.hi)).padEnd(18)} ` +
        `${String(e.cps).padStart(5)} ${String(e.occ).padStart(10)}`,
    );
  }

  // ── Basic Latin 内的子区分布（本语料全部落在该区块，故细化到子区才有分辨力）
  const bySub = new Map();
  for (const cp of cps) {
    const s = subrangeOf(cp);
    if (!s) continue;
    let e = bySub.get(s);
    if (!e) bySub.set(s, (e = { cps: 0, occ: 0 }));
    e.cps++;
    e.occ += inv.get(cp).occ;
  }
  console.log("");
  console.log("== Basic Latin 内子区分布（按全行出现次数降序） ==");
  console.log("  子区                            码点   出现次数");
  for (const [name, e] of [...bySub.entries()].sort((a, b) => b[1].occ - a[1].occ)) {
    console.log(`  ${name.padEnd(30)} ${String(e.cps).padStart(5)} ${String(e.occ).padStart(10)}`);
  }

  // ── 高频码点（体现语料实际用什么字符）
  const topOcc = cps.slice().sort((a, b) => inv.get(b).occ - inv.get(a).occ || a - b);
  console.log("");
  console.log(`== 出现次数最高的 ${args.top} 个码点 ==`);
  console.log("  码点     字符          区块                          出现次数  所属去重名数");
  for (const cp of topOcc.slice(0, args.top)) {
    const e = inv.get(cp);
    console.log(
      `  ${fmtCp(cp)}  ${fmtChar(cp).padEnd(12)}  ${blockOf(cp).padEnd(28)} ` +
        `${String(e.occ).padStart(8)}  ${String(e.names.size).padStart(10)}`,
    );
  }

  // ── 0-255 覆盖判定（这就是「city 标签能否用 0-255.pbf 渲染」的判据）
  const inside = cps.filter((c) => c <= 0xff);
  const outside = cps.filter((c) => c > 0xff);
  console.log("");
  console.log("== 对 Latin-1 范围 U+0000..U+00FF 的覆盖判定 ==");
  console.log(`  落在 0-255 内的码点           ${inside.length} / ${cps.length}`);
  console.log(`  落在 0-255 外的码点           ${outside.length} / ${cps.length}`);
  if (outside.length === 0) {
    console.log("  结论                          语料完全落在 U+0000..U+00FF 内");
  } else {
    console.log("  缺失码点清单：");
    for (const cp of outside) {
      console.log(
        `    ${fmtCp(cp)}  ${fmtChar(cp)}  ${blockOf(cp)}  出现 ${inv.get(cp).occ} 次`,
      );
    }
  }

  // ── 需要的字形分片（{range}.pbf）集合
  const reqRanges = [...new Set(cps.map(rangeLabel))].sort(
    (a, b) => Number.parseInt(a, 10) - Number.parseInt(b, 10),
  );
  const reqRangesAll = [...new Set([...invAll.keys()].map(rangeLabel))].sort(
    (a, b) => Number.parseInt(a, 10) - Number.parseInt(b, 10),
  );
  console.log("");
  console.log("== 渲染所需字形分片 {range}.pbf ==");
  console.log(`  City 语料所需                ${reqRanges.join(", ")}`);
  console.log(`  全部 POI 语料所需            ${reqRangesAll.join(", ")}`);

  // ── 辅助口径：全部 POI 名（含 company 等非城市名），用于说明上界
  const cpsAll = [...invAll.keys()].sort((a, b) => a - b);
  const outsideAll = cpsAll.filter((c) => c > 0xff);
  console.log("");
  console.log("== 辅助口径：poi 表全部 type 的 name（含非城市名） ==");
  console.log(`  distinct 码点数               ${cpsAll.length}`);
  console.log(`  超出 U+00FF 的码点数          ${outsideAll.length}`);
  if (outsideAll.length) {
    console.log(`  超出清单                      ${outsideAll.map(fmtCp).join(", ")}`);
  }

  // ── 标签文本形态判定：瓦片 city 图层的 name 是**城市 token**（TileBuilder.cs:101
  //    使用 c.City），而非人类可读的城市名。这决定了「字形覆盖」与「标签可读性」
  //    是两个独立断言，不能以字形覆盖推及标签完整性。
  const TOKEN_PATTERN = /^[a-z_]+$/;
  const nonToken = distinct.filter((n) => !TOKEN_PATTERN.test(n));
  console.log("");
  console.log("== 标签文本形态 ==");
  console.log(`  匹配 /^[a-z_]+$/ 的去重名     ${distinct.length - nonToken.length} / ${distinct.length}`);
  if (nonToken.length) {
    console.log(`  不匹配清单（前 20）           ${nonToken.slice(0, 20).join(", ")}`);
  }

  if (args.allCodepoints) {
    console.log("");
    console.log("== 全量码点（主口径） ==");
    for (const cp of cps) {
      const e = inv.get(cp);
      console.log(`  ${fmtCp(cp)}\t${fmtChar(cp)}\t${blockOf(cp)}\t${e.occ}\t${e.names.size}`);
    }
  }

  // ── 可复现记录：脚本自身与输入的摘要
  const selfHash = createHash("sha256")
    .update(readFileSync(fileURLToPath(import.meta.url)))
    .digest("hex");
  const dbHash = createHash("sha256").update(dbBytes).digest("hex");
  console.log("");
  console.log("== 可复现记录 ==");
  console.log(`  script sha256                 ${selfHash}`);
  console.log(`  search.db sha256              ${dbHash}`);
  console.log(`  search.db bytes               ${dbBytes.length}`);
  console.log(`  node                          ${process.version}`);

  if (args.json) {
    const payload = {
      schema: "ets2nav-city-charset-inventory/1",
      dataset_dir: args.dataset,
      source: {
        file: "search.db",
        sqlite_magic_matched: true,
        sha256: dbHash,
        bytes: dbBytes.length,
        table: "poi",
        filter: "type = 'City'",
      },
      corpus: {
        city_rows: cityRows.length,
        city_distinct_names: distinct.length,
        manifest_stats_cities: manifestCities,
        all_poi_rows: allRows.length,
        all_poi_distinct_names: allDistinct.length,
      },
      label_form: {
        note:
          "city 图层的 name 取自 CityItem.City（map-compiler/src/ScsVectorTiles/TileBuilder.cs:101），" +
          "即城市 token，而非 city_name_localized 的人类可读名称",
        token_pattern: "^[a-z_]+$",
        matching_distinct_names: distinct.length - nonToken.length,
        non_matching_distinct_names: nonToken,
      },
      inventory: {
        distinct_codepoints: cps.length,
        min_codepoint: cps[0],
        max_codepoint: maxCp,
        non_ascii: cps.filter((c) => c > 0x7f).length,
        outside_latin1: outside.map(fmtCp),
        required_glyph_ranges: reqRanges,
        blocks: Object.fromEntries(
          blockRows.map(([name, e]) => [
            name,
            { lo: fmtCp(e.lo), hi: fmtCp(e.hi), codepoints: e.cps, occurrences: e.occ },
          ]),
        ),
        ascii_subranges: Object.fromEntries(
          [...bySub.entries()].map(([name, e]) => [name, { codepoints: e.cps, occurrences: e.occ }]),
        ),
        codepoints: cps.map((cp) => ({
          cp: fmtCp(cp),
          char: fmtChar(cp),
          block: blockOf(cp),
          occurrences: inv.get(cp).occ,
          distinct_names: inv.get(cp).names.size,
        })),
      },
      auxiliary_all_poi_names: {
        distinct_codepoints: cpsAll.length,
        outside_latin1: outsideAll.map(fmtCp),
        required_glyph_ranges: reqRangesAll,
      },
      generator: {
        script: "scripts/analyze-city-charset.mjs",
        script_sha256: selfHash,
        node: process.version,
      },
    };
    writeFileSync(args.json, `${JSON.stringify(payload, null, 2)}\n`, "utf8");
    console.log(`  清单 JSON 已写入             ${args.json}`);
  }

  process.exit(EXIT_OK);
}

main();
