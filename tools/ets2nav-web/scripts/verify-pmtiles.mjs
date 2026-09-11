#!/usr/bin/env node
// PMTiles 互操作验证（P4R Batch 1.5 §7）——独立读取器 Gate。
//
// 用官方 `pmtiles` npm 包（与 Web 前端锁定的同一版本）作为独立 reader，
// 读取由 C# 正式 PmtilesWriter 生成的归档，验证：
//   1. header 可解析且字段与规范一致；
//   2. metadata 可解析且 type = baselayer；
//   3. 每个（或抽样）z/x/y 都能取回瓦片，且解压后字节与该瓦片原始 MVT 完全一致；
//   4. 官方 zxyToTileId 与本项目 Hilbert 实现给出的 tile_id 逐项一致；
//   5. 不存在的瓦片返回 undefined（不得误命中相邻条目）。
//
// 本脚本刻意不使用 writer 侧任何代码解码，也不 mock reader。
//
// 用法：
//   node scripts/verify-pmtiles.mjs <archive.pmtiles> [--sample N]
//   node scripts/verify-pmtiles.mjs http://host:port/map.pmtiles --expect <sidecar.json> [--sample N]
//
// 传 http(s) URL 时走 pmtiles 自带的 FetchSource（真实 HTTP Range 路径），
// 用于验证「nav-server 字节服务 + 官方 reader」联调；此时需显式给出 sidecar。

import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { PMTiles, FileSource, zxyToTileId, Compression, TileType } from "pmtiles";

const target = process.argv[2];
if (!target) {
  console.error("用法: node scripts/verify-pmtiles.mjs <archive.pmtiles|http-url> [--expect <sidecar>] [--sample N]");
  process.exit(2);
}
const isUrl = /^https?:\/\//.test(target);
const sampleArg = process.argv.indexOf("--sample");
const sampleSize = sampleArg > 0 ? Number(process.argv[sampleArg + 1]) : 0;
const expectArg = process.argv.indexOf("--expect");
const expectPath = expectArg > 0 ? process.argv[expectArg + 1] : `${target}.expect.json`;
// 无 sidecar 时进入结构化模式：只做与 sidecar 无关的规范/可读性校验。
// 适用于 map-inspector 直接产出的真实归档（没有夹具期望值）。
const structuralOnly = !existsSync(expectPath);

const failures = [];
function check(name, ok, detail = "") {
  console.log(`[${ok ? "PASS" : "FAIL"}] ${name}${detail ? ` — ${detail}` : ""}`);
  if (!ok) failures.push(name);
}

const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

const expect = structuralOnly ? null : JSON.parse(readFileSync(expectPath, "utf8"));

// 本地文件走 FileSource（Blob 风格接口）；URL 走 FetchSource —— PMTiles 构造函数
// 只在实参为 string 时自动包 FetchSource，故本地路径必须显式包。
let archive;
if (isUrl) {
  archive = new PMTiles(target);
} else {
  const bytes = readFileSync(target);
  const blob = new Blob([bytes]);
  archive = new PMTiles(new FileSource({ name: target, slice: (s, e) => blob.slice(s, e) }));
}
console.log(`读取源: ${isUrl ? "HTTP (FetchSource) " : "文件 (FileSource) "}${target}`);
console.log(`模式: ${structuralOnly ? "结构化（无 sidecar）" : "夹具比对"}\n`);

// ── 1. header ────────────────────────────────────────────────────────────────
const header = await archive.getHeader();
check("header 可解析（magic/version）", header.specVersion === 3, `specVersion=${header.specVersion}`);
check(
  "compression 枚举：internal=None(1) / tile=Gzip(2)",
  header.internalCompression === Compression.None && header.tileCompression === Compression.Gzip,
  `internal=${header.internalCompression} tile=${header.tileCompression}`,
);
check("tileType = MVT(1)", header.tileType === TileType.Mvt, `tileType=${header.tileType}`);
if (expect) {
  check(
    "min/max zoom 正确",
    header.minZoom === expect.minZoom && header.maxZoom === expect.maxZoom,
    `${header.minZoom}-${header.maxZoom}（期望 ${expect.minZoom}-${expect.maxZoom}）`,
  );
  check(
    "tile entry count 与夹具一致",
    header.numTileEntries === expect.tileEntryCount,
    `${header.numTileEntries}（期望 ${expect.tileEntryCount}）`,
  );
} else {
  check(
    "min/max zoom 处于合法范围",
    header.minZoom >= 0 && header.maxZoom >= header.minZoom && header.maxZoom <= 26,
    `${header.minZoom}-${header.maxZoom}`,
  );
  check("tile entry count 非零", header.numTileEntries > 0, `entries=${header.numTileEntries}`);
}
check(
  "counts 自洽（addressed == entries == contents，无去重）",
  header.numAddressedTiles === header.numTileEntries && header.numTileContents === header.numTileEntries,
  `addressed=${header.numAddressedTiles} entries=${header.numTileEntries} contents=${header.numTileContents}`,
);
check(
  "root directory 落在 header 127 + 16384 限制内",
  header.rootDirectoryOffset + header.rootDirectoryLength <= 16384,
  `off=${header.rootDirectoryOffset} len=${header.rootDirectoryLength} sum=${header.rootDirectoryOffset + header.rootDirectoryLength}`,
);
if (expect) {
  const boundsOk =
    Math.abs(header.minLon - expect.bounds[0]) < 1e-6 &&
    Math.abs(header.minLat - expect.bounds[1]) < 1e-6 &&
    Math.abs(header.maxLon - expect.bounds[2]) < 1e-6 &&
    Math.abs(header.maxLat - expect.bounds[3]) < 1e-6;
  check("bounds 正确（int32 E7）", boundsOk,
    `[${header.minLon},${header.minLat},${header.maxLon},${header.maxLat}]`);
  check("center zoom 正确", header.centerZoom === expect.centerZoom, `centerZoom=${header.centerZoom}`);
} else {
  check(
    "bounds 为有效非退化范围",
    header.minLon < header.maxLon && header.minLat < header.maxLat,
    `[${header.minLon},${header.minLat},${header.maxLon},${header.maxLat}]`,
  );
}
if (header.leafDirectoryLength > 0) {
  console.log(`[INFO] 该归档含 leaf directory：offset=${header.leafDirectoryOffset} length=${header.leafDirectoryLength}`);
}

// ── 2. metadata ──────────────────────────────────────────────────────────────
const meta = await archive.getMetadata();
check("metadata 可解析（JSON）", meta !== null && typeof meta === "object");
check("metadata.type = baselayer", meta.type === "baselayer", `type=${meta.type}`);
const layerIds = (meta.vector_layers || []).map((l) => l.id);
check(
  "metadata.vector_layers 含 road/junction/city/poi",
  ["road", "junction", "city", "poi"].every((id) => layerIds.includes(id)),
  `layers=${layerIds.join(",")}`,
);

// ── 3~5. 瓦片可读性 ────────────────────────────────────────────────────────
if (expect) {
  // 夹具模式：官方 zxyToTileId 与本项目 tile_id 逐项比对 + 字节级比对
  let idMismatch = 0;
  let firstMismatch = "";
  for (const t of expect.tiles) {
    const official = zxyToTileId(t.z, t.x, t.y);
    if (official !== t.tileId) {
      idMismatch++;
      if (!firstMismatch) firstMismatch = `z=${t.z} x=${t.x} y=${t.y} 官方=${official} 本项目=${t.tileId}`;
    }
  }
  check(
    `TileID 与官方 zxyToTileId 逐项一致（${expect.tiles.length} 项）`,
    idMismatch === 0,
    idMismatch ? `${idMismatch} 项不一致，首个：${firstMismatch}` : "全部一致",
  );

  const targets = sampleSize > 0 && sampleSize < expect.tiles.length
    ? shuffleDeterministic(expect.tiles).slice(0, sampleSize)
    : expect.tiles;

  let readOk = 0;
  let byteMismatch = 0;
  let missing = 0;
  let firstError = "";
  for (const t of targets) {
    let tile;
    try {
      tile = await archive.getZxy(t.z, t.x, t.y);
    } catch (e) {
      missing++;
      if (!firstError) firstError = `z=${t.z} x=${t.x} y=${t.y} 抛错: ${e.message}`;
      continue;
    }
    if (!tile || !tile.data) {
      missing++;
      if (!firstError) firstError = `z=${t.z} x=${t.x} y=${t.y} 返回空`;
      continue;
    }
    const digest = sha256(Buffer.from(tile.data));
    if (digest !== t.mvtSha256) {
      byteMismatch++;
      if (!firstError) firstError = `z=${t.z} x=${t.x} y=${t.y} 字节不一致 ${digest} != ${t.mvtSha256}`;
      continue;
    }
    readOk++;
  }
  check(
    `瓦片解压后字节与原始 MVT 完全一致（${targets.length} 块）`,
    readOk === targets.length && byteMismatch === 0 && missing === 0,
    `ok=${readOk} missing=${missing} mismatch=${byteMismatch}${firstError ? " | " + firstError : ""}`,
  );

  const present = new Set(expect.tiles.map((t) => `${t.z}/${t.x}/${t.y}`));
  let absentProbed = 0;
  let falseHit = 0;
  for (const t of expect.tiles.slice(0, 40)) {
    const key = `${t.z}/${t.x + 1}/${t.y}`;
    if (present.has(key) || t.x + 1 >= 1 << t.z) continue;
    absentProbed++;
    const tile = await archive.getZxy(t.z, t.x + 1, t.y);
    if (tile && tile.data && tile.data.byteLength > 0) falseHit++;
  }
  check(
    `未收录瓦片返回空（探测 ${absentProbed} 块）`,
    absentProbed > 0 && falseHit === 0,
    `误命中=${falseHit}`,
  );
} else {
  // 结构化模式：按 header 的 bounds/zoom 扫描 z/x/y 空间，全部经官方 reader 取回。
  const found = [];
  const layersSeen = new Set();
  const decodeErrors = [];
  for (let z = header.minZoom; z <= header.maxZoom; z++) {
    const n = 1 << z;
    const clamp = (v) => Math.max(0, Math.min(n - 1, v));
    const x0 = clamp(lonToTileX(header.minLon, z));
    const x1 = clamp(lonToTileX(header.maxLon, z));
    const y0 = clamp(latToTileY(header.maxLat, z)); // 纬度越大 y 越小
    const y1 = clamp(latToTileY(header.minLat, z));
    for (let x = Math.min(x0, x1); x <= Math.max(x0, x1); x++) {
      for (let y = Math.min(y0, y1); y <= Math.max(y0, y1); y++) {
        let tile;
        try {
          tile = await archive.getZxy(z, x, y);
        } catch (e) {
          decodeErrors.push(`z=${z} x=${x} y=${y}: ${e.message}`);
          continue;
        }
        if (!tile || !tile.data || tile.data.byteLength === 0) continue;
        found.push(`${z}/${x}/${y}`);
        try {
          for (const name of mvtLayerNames(new Uint8Array(tile.data))) layersSeen.add(name);
        } catch (e) {
          decodeErrors.push(`z=${z} x=${x} y=${y} MVT 解析失败: ${e.message}`);
        }
      }
    }
  }
  check(`在 bounds 内取回至少一块瓦片`, found.length > 0, `取回 ${found.length} 块`);
  check(`全部瓦片 MVT 可解析且为合法图层`, decodeErrors.length === 0,
    decodeErrors.length ? decodeErrors.slice(0, 3).join("; ") : `${found.length} 块全部解析通过`);
  check(
    `实际瓦片内容覆盖 metadata 声明的全部图层`,
    ["road", "junction", "city", "poi"].every((id) => layersSeen.has(id)),
    `实际图层=${[...layersSeen].sort().join(",")}`,
  );
  // bounds 之外的瓦片必须取不到（防止目录误命中）
  const outsideZ = header.maxZoom;
  const outsideX = Math.min((1 << outsideZ) - 1, lonToTileX(header.maxLon, outsideZ) + 3);
  const outside = await archive.getZxy(outsideZ, outsideX, latToTileY(header.maxLat, outsideZ));
  check("bounds 之外的瓦片返回空", !outside || !outside.data || outside.data.byteLength === 0);
}

function lonToTileX(lon, z) {
  return Math.floor(((lon + 180) / 360) * (1 << z));
}
function latToTileY(lat, z) {
  const r = (lat * Math.PI) / 180;
  return Math.floor(((1 - Math.log(Math.tan(r) + 1 / Math.cos(r)) / Math.PI) / 2) * (1 << z));
}

/** 从 MVT 字节中提取图层名（最小 protobuf 解析；packed 字段按字节长度推进）。 */
function mvtLayerNames(buf) {
  let pos = 0;
  const rv = () => {
    let r = 0, s = 0, b;
    do { b = buf[pos++]; r |= (b & 0x7f) << s; s += 7; } while (b & 0x80);
    return r >>> 0;
  };
  const skip = (w) => {
    if (w === 0) rv();
    else if (w === 1) pos += 8;
    else if (w === 2) { const l = rv(); pos += l; }
    else if (w === 5) pos += 4;
    else throw new Error(`未知 wire type ${w} @${pos}`);
  };
  const names = [];
  while (pos < buf.length) {
    const tag = rv();
    const field = tag >> 3, wire = tag & 7;
    if (field !== 3 || wire !== 2) { skip(wire); continue; }
    const layerEnd = pos + rv();
    while (pos < layerEnd) {
      const t2 = rv();
      const f2 = t2 >> 3, w2 = t2 & 7;
      if (f2 === 1 && w2 === 2) {
        const l = rv();
        names.push(new TextDecoder().decode(buf.subarray(pos, pos + l)));
        pos += l;
      } else skip(w2);
    }
  }
  if (names.length === 0) throw new Error("图层数为 0");
  return names;
}

/** 确定性「抽样」：按 tile_id 等距取样，保证可复现。 */
function shuffleDeterministic(list) {
  const step = Math.max(1, Math.floor(list.length / (sampleSize || list.length)));
  return list.filter((_, i) => i % step === 0);
}

console.log("");
if (failures.length) {
  console.log(`PMTILES INTEROP FAIL: ${failures.length} 项 — ${failures.join("; ")}`);
  process.exit(1);
}
console.log("PMTILES INTEROP PASS");
