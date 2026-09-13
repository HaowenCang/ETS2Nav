// ETS2Nav 发布产物层断言（P4R 发布工程 §16）。
//
// 被判定的对象是**将要上传的字节**：ZIP 归档本身，以及由它解压出的产物树。
// desktop-lifecycle.mjs 判定的是「这个 bundle 能否作为产品运行」；本文件只补充它
// 无法表达的五组归档层性质：
//
//   A-01 归档结构：顶层恰好是单一目录 ETS2Nav/，条目集合完整，且不含禁止内容
//        （target/、node_modules/、.git/、*.pdb、playwright-report/、test-results/、
//        vendor/scs_sdk_1_14.zip），条目名不得是绝对路径或含 ..
//   A-02 档位自洽：产物文件名里的 <profile> 与 bundle-manifest.json 的 profile、
//        以及 basemap/fonts 的 present 标记三者必须互相蕴含
//   A-03 许可文件：LICENSE 与 THIRD_PARTY_NOTICES.txt 存在且非空
//   A-04 插件身份：解压树内两个 DLL 的 SHA-256 与 release-manifest.json 的 plugins 数组一致
//   A-05 产物自述 vs 真实字节：bundle-manifest.json 声明的两个二进制 sha256/bytes、
//        web/ 与 data/europe-v5/ 的 files/bytes/tree_sha256 必须与磁盘一致
//
// 关于 A-05 的树摘要：权威实现是 desktop/scripts/BundleCommon.ps1::Get-TreeDigest，
// 但**照抄那个实现来核对它自己的输出没有意义**，所以这里按同一份算法定义独立实现一次，
// 用来做交叉核对。两份实现对同一棵树必须给出同一个值；不一致时本脚本以 FAIL 结束
// 并同时打印两个值——那是可诊断的失败，而不是可以被吞掉的差异。
//
// 归档确定性（条目按序数序、时间戳统一）也在此顺带断言，它是 §17 的可复现性前提。
//
// 退出码：0 全部 PASS；1 有 FAIL；3 前置条件（ZIP / 解压树 / release-manifest.json 缺失）；
// 4 harness 自身错误。绝不因「未执行」而退出 0。

import { closeSync, createReadStream, existsSync, openSync, readSync, readdirSync, statSync } from "node:fs";
import { createHash } from "node:crypto";
import { join, resolve } from "node:path";

const args = process.argv.slice(2);
function argValue(name, dflt = null) {
  const i = args.indexOf(name);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : dflt;
}

const ZIP = argValue("--zip");
const EXTRACT = argValue("--extract");
const RELEASE_MANIFEST = argValue("--release-manifest");
const EXPECT_PROFILE = (argValue("--expect-profile", "CORE") || "CORE").toUpperCase();

const FAIL = [];
let passCount = 0;
let notRun = 0;

function check(name, ok, detail = "") {
  if (ok) {
    passCount += 1;
    console.log(`  [PASS] ${name}${detail ? ` - ${detail}` : ""}`);
  } else {
    FAIL.push(name);
    console.log(`  [FAIL] ${name}${detail ? ` - ${detail}` : ""}`);
  }
}
function notVerified(name, why) {
  notRun += 1;
  console.log(`  [NOT RUN] ${name} - ${why}`);
}
function stop(code, message) {
  console.error(message);
  process.exit(code);
}

// ── ZIP 中央目录（不依赖任何第三方库）────────────────────────────────────────
const EOCD_SIG = 0x06054b50;
const CD_SIG = 0x02014b50;

function readCentralDirectory(zipPath) {
  const size = statSync(zipPath).size;
  const tailLen = Math.min(size, 65535 + 22);
  const fd = openSync(zipPath, "r");
  let tail;
  let cd;
  let count;
  let cdOffset;
  let cdSize;
  try {
    tail = Buffer.alloc(tailLen);
    readSync(fd, tail, 0, tailLen, size - tailLen);
    let eocd = -1;
    for (let i = tail.length - 22; i >= 0; i--) {
      if (tail.readUInt32LE(i) === EOCD_SIG) { eocd = i; break; }
    }
    if (eocd < 0) throw new Error("找不到 ZIP 中央目录结束记录（EOCD）");
    count = tail.readUInt16LE(eocd + 10);
    cdSize = tail.readUInt32LE(eocd + 12);
    cdOffset = tail.readUInt32LE(eocd + 16);
    if (cdOffset === 0xffffffff || count === 0xffff || cdSize === 0xffffffff) {
      throw new Error("归档使用了 ZIP64 结构；本 harness 未实现 ZIP64（本发布件不应产生 ZIP64）");
    }
    cd = Buffer.alloc(cdSize);
    readSync(fd, cd, 0, cdSize, cdOffset);
  } finally {
    closeSync(fd);
  }
  const entries = [];
  let p = 0;
  for (let i = 0; i < count; i++) {
    if (cd.readUInt32LE(p) !== CD_SIG) throw new Error(`中央目录第 ${i} 项签名非法`);
    entries.push({
      name: cd.slice(p + 46, p + 46 + cd.readUInt16LE(p + 28)).toString("utf8"),
      flags: cd.readUInt16LE(p + 8),
      method: cd.readUInt16LE(p + 10),
      dosTime: cd.readUInt16LE(p + 12),
      dosDate: cd.readUInt16LE(p + 14),
      crc: cd.readUInt32LE(p + 16),
      compSize: cd.readUInt32LE(p + 20),
      uncompSize: cd.readUInt32LE(p + 24),
      externalAttr: cd.readUInt32LE(p + 38),
    });
    p += 46 + cd.readUInt16LE(p + 28) + cd.readUInt16LE(p + 30) + cd.readUInt16LE(p + 32);
  }
  return entries;
}

// ── 树摘要（与 BundleCommon.ps1::Get-TreeDigest 同一算法定义的独立实现）──────
function walkRel(root, prefix, out) {
  for (const name of readdirSync(join(root, prefix), { withFileTypes: true })) {
    const rel = prefix ? `${prefix}/${name.name}` : name.name;
    if (name.isDirectory()) walkRel(root, rel, out);
    else if (name.isFile()) out.push({ rel, size: statSync(join(root, rel)).size });
  }
  return out;
}
function sha256File(path) {
  return new Promise((ok, bad) => {
    const h = createHash("sha256");
    createReadStream(path).on("data", (d) => h.update(d)).on("error", bad).on("end", () => ok(h.digest("hex")));
  });
}
async function treeDigest(root) {
  const files = walkRel(root, "", []).sort((a, b) => (a.rel < b.rel ? -1 : a.rel > b.rel ? 1 : 0));
  const h = createHash("sha256");
  for (const f of files) {
    h.update(Buffer.from(f.rel, "utf8"));
    h.update(Buffer.from([0]));
    h.update(Buffer.from(String(f.size), "utf8"));
    h.update(Buffer.from([0]));
    h.update(Buffer.from(await sha256File(join(root, f.rel)), "utf8"));
    h.update(Buffer.from("\n"));
  }
  return { digest: h.digest("hex"), files: files.length, bytes: files.reduce((s, f) => s + f.size, 0) };
}

// ── 主流程 ───────────────────────────────────────────────────────────────────
async function main() {
  if (!ZIP) stop(3, "PRECONDITION FAILURE: 缺少 --zip");
  if (!EXTRACT) stop(3, "PRECONDITION FAILURE: 缺少 --extract");
  if (!RELEASE_MANIFEST) {
    stop(3, "PRECONDITION FAILURE: 缺少 --release-manifest（插件身份核对依赖外层清单；该检查不能跳过）");
  }
  if (!existsSync(ZIP)) stop(3, `PRECONDITION FAILURE: ZIP 不存在: ${ZIP}`);
  if (!existsSync(EXTRACT)) stop(3, `PRECONDITION FAILURE: 解压目录不存在: ${EXTRACT}`);
  if (!existsSync(RELEASE_MANIFEST)) stop(3, `PRECONDITION FAILURE: release-manifest.json 不存在: ${RELEASE_MANIFEST}`);

  const zipLeaf = ZIP.replace(/\\/g, "/").split("/").pop();
  console.log("=== ETS2Nav 发布产物层断言（§16）===");
  console.log(`zip      : ${resolve(ZIP)}`);
  console.log(`extract  : ${resolve(EXTRACT)}`);
  console.log(`manifest : ${resolve(RELEASE_MANIFEST)}`);
  console.log(`expect   : profile=${EXPECT_PROFILE}`);

  const rel = JSON.parse((await import("node:fs")).readFileSync(RELEASE_MANIFEST, "utf8"));
  const topLevel = readdirSync(EXTRACT, { withFileTypes: true }).map((d) => d.name);
  if (!topLevel.includes("ETS2Nav")) stop(3, `PRECONDITION FAILURE: 解压根下没有 ETS2Nav/（实际：${topLevel.join(", ")}）`);
  const bundleRoot = join(EXTRACT, "ETS2Nav");
  const bmPath = join(bundleRoot, "bundle-manifest.json");
  if (!existsSync(bmPath)) stop(3, "PRECONDITION FAILURE: 解压树缺少 bundle-manifest.json");
  const bm = JSON.parse((await import("node:fs")).readFileSync(bmPath, "utf8"));

  // ── A-01 归档结构 ─────────────────────────────────────────────────────────
  console.log("\n-- A-01 归档结构 --");
  const entries = readCentralDirectory(ZIP);
  const names = entries.map((e) => e.name);
  const badTop = names.filter((n) => !n.startsWith("ETS2Nav/"));
  check("所有条目都位于单一顶层目录 ETS2Nav/ 之下", badTop.length === 0, badTop.slice(0, 3).join(", "));
  const topSegments = [...new Set(names.map((n) => n.split("/")[0]))];
  check("顶层恰好一个目录项", topSegments.length === 1 && topSegments[0] === "ETS2Nav", topSegments.join(", "));
  const dirEntries = entries.filter((e) => e.name.endsWith("/"));
  check("归档不含目录条目（条目集合只有文件）", dirEntries.length === 0, dirEntries.map((e) => e.name).join(", "));

  const forbidden = [
    /(^|\/)(target|node_modules|\.git|playwright-report|test-results)(\/|$)/i,
    /\.pdb$/i,
    /(^|\/)vendor\/scs_sdk_1_14\.zip$/i,
  ];
  const forbiddenHits = names.filter((n) => forbidden.some((re) => re.test(n)));
  check("归档不含禁止内容（target/、node_modules/、.git/、*.pdb、playwright-report/、test-results/、vendor/scs_sdk_1_14.zip）",
    forbiddenHits.length === 0, forbiddenHits.slice(0, 5).join(", "));
  const unsafe = names.filter((n) => n.startsWith("/") || /^[A-Za-z]:/.test(n) || n.split("/").includes(".."));
  check("条目名不是绝对路径、不含 ..", unsafe.length === 0, unsafe.slice(0, 3).join(", "));
  const dupes = names.filter((n, i) => names.indexOf(n) !== i);
  check("条目名不重复", dupes.length === 0, [...new Set(dupes)].slice(0, 3).join(", "));

  const required = [
    "ETS2Nav/ets2nav-desktop.exe",
    "ETS2Nav/nav-core-cli.exe",
    "ETS2Nav/bundle-manifest.json",
    "ETS2Nav/LICENSE",
    "ETS2Nav/THIRD_PARTY_NOTICES.txt",
    "ETS2Nav/plugins/README.txt",
    "ETS2Nav/plugins/scs-nav-bridge.dll",
    "ETS2Nav/plugins/semaphore-bridge.dll",
    "ETS2Nav/web/index.html",
    "ETS2Nav/data/europe-v5/manifest.json",
  ];
  const missing = required.filter((n) => !names.includes(n));
  check("必需条目齐备", missing.length === 0, missing.join(", "));

  // 条目序数有序 + 时间戳统一（§17 的归档确定性前提）
  let ordered = true;
  for (let i = 1; i < names.length; i++) if (!(names[i - 1] < names[i])) { ordered = false; break; }
  check("条目按序数序写入", ordered, `${names.length} 个条目`);
  const stamps = [...new Set(entries.map((e) => `${e.dosDate}:${e.dosTime}`))];
  check("所有条目的 DOS 时间戳一致（固定瞬时）", stamps.length === 1, stamps.join(", "));
  check("条目使用固定压缩方法（deflate）", entries.every((e) => e.method === 8), [...new Set(entries.map((e) => e.method))].join(", "));

  // ── A-02 档位自洽 ─────────────────────────────────────────────────────────
  console.log("\n-- A-02 档位自洽（文件名 / 清单 / 资源存在性）--");
  const m = /^ETS2Nav-(.+)-windows-x64-(core|full)\.zip$/.exec(zipLeaf);
  check("产物文件名符合 ETS2Nav-<version>-windows-x64-<profile>.zip", m !== null, zipLeaf);
  if (m) {
    check("文件名版本与 bundle-manifest.json 的 app_version 一致", m[1] === bm.app_version, `name=${m[1]} manifest=${bm.app_version}`);
    check("文件名 profile 与 bundle-manifest.json 的 profile 一致",
      m[2].toUpperCase() === String(bm.profile).toUpperCase(), `name=${m[2]} manifest=${bm.profile}`);
  }
  const allPresent = Boolean(bm.basemap?.present) && Boolean(bm.fonts?.present);
  check("profile=FULL 蕴含 basemap.present 且 fonts.present", String(bm.profile) === "FULL" ? allPresent : true,
    `profile=${bm.profile} basemap=${bm.basemap?.present} fonts=${bm.fonts?.present}`);
  check("profile=CORE 蕴含 basemap 或 fonts 缺席", String(bm.profile) === "CORE" ? !allPresent : true,
    `profile=${bm.profile} basemap=${bm.basemap?.present} fonts=${bm.fonts?.present}`);
  check(`本次环境期望 profile=${EXPECT_PROFILE}`, String(bm.profile).toUpperCase() === EXPECT_PROFILE,
    `实际=${bm.profile}`);
  if (String(bm.profile).toUpperCase() !== EXPECT_PROFILE && allPresent) {
    console.log("        说明：profile 为 FULL 时须确认底图/字形是真实资源而不是源码目录里的开发桩件。");
  }

  // 资源存在性与磁盘一致（present 标记不得与磁盘矛盾）
  const basemapOnDisk = existsSync(join(bundleRoot, "web", "map.pmtiles"));
  check("basemap.present 与磁盘一致", Boolean(bm.basemap?.present) === basemapOnDisk,
    `manifest=${bm.basemap?.present} disk=${basemapOnDisk}`);
  const fontsOnDisk = existsSync(join(bundleRoot, "web", "vendor", "fonts"));
  check("fonts.present 与磁盘一致", Boolean(bm.fonts?.present) === fontsOnDisk,
    `manifest=${bm.fonts?.present} disk=${fontsOnDisk}`);

  // ── A-03 许可文件 ─────────────────────────────────────────────────────────
  console.log("\n-- A-03 许可与第三方声明 --");
  for (const f of ["LICENSE", "THIRD_PARTY_NOTICES.txt"]) {
    const p = join(bundleRoot, f);
    const ok = existsSync(p) && statSync(p).size > 0;
    check(`${f} 存在且非空`, ok, ok ? `${statSync(p).size} B` : "缺失或为空");
  }

  // ── A-04 插件身份 ─────────────────────────────────────────────────────────
  console.log("\n-- A-04 插件身份（对照 release-manifest.json）--");
  const manifestPlugins = Array.isArray(rel.plugins) ? rel.plugins : [];
  if (manifestPlugins.length === 0) check("release-manifest.json 记录了插件", false, "plugins 为空");
  for (const p of manifestPlugins) {
    const disk = join(bundleRoot, "plugins", p.name);
    if (!existsSync(disk)) { check(`插件 ${p.name} 存在于解压树`, false, disk); continue; }
    const id = { sha256: await sha256File(disk), bytes: statSync(disk).size };
    check(`插件 ${p.name} sha256 与 release-manifest.json 一致`, id.sha256 === p.sha256,
      `disk=${id.sha256.slice(0, 16)}... manifest=${String(p.sha256).slice(0, 16)}...`);
    check(`插件 ${p.name} bytes 与 release-manifest.json 一致`, id.bytes === p.bytes, `disk=${id.bytes} manifest=${p.bytes}`);
  }
  const diskDlls = existsSync(join(bundleRoot, "plugins"))
    ? readdirSync(join(bundleRoot, "plugins")).filter((n) => n.toLowerCase().endsWith(".dll")).sort()
    : [];
  check("插件目录内的 DLL 集合与清单一致",
    diskDlls.length === manifestPlugins.length && diskDlls.every((n) => manifestPlugins.some((p) => p.name === n)),
    `disk=[${diskDlls.join(", ")}] manifest=[${manifestPlugins.map((p) => p.name).join(", ")}]`);

  // ── A-05 bundle 自述 vs 真实字节 ──────────────────────────────────────────
  console.log("\n-- A-05 bundle 自述 vs 真实字节 --");
  for (const [key, file] of [["desktop_exe", "ets2nav-desktop.exe"], ["sidecar", "nav-core-cli.exe"]]) {
    const p = join(bundleRoot, file);
    if (!existsSync(p)) { check(`${key} 存在`, false, file); continue; }
    const sha = await sha256File(p);
    const bytes = statSync(p).size;
    check(`${key} sha256 与 bundle-manifest.json 一致`, sha === bm[key].sha256,
      `disk=${sha.slice(0, 16)}... manifest=${String(bm[key].sha256).slice(0, 16)}...`);
    check(`${key} bytes 与 bundle-manifest.json 一致`, bytes === bm[key].bytes, `disk=${bytes} manifest=${bm[key].bytes}`);
  }

  const webDir = join(bundleRoot, "web");
  const webTree = await treeDigest(webDir);
  check("web tree_sha256 与 bundle-manifest.json 一致", webTree.digest === bm.web.tree_sha256,
    `disk=${webTree.digest.slice(0, 16)}... manifest=${String(bm.web.tree_sha256).slice(0, 16)}...`);
  check("web files/bytes 与 bundle-manifest.json 一致",
    webTree.files === bm.web.files && webTree.bytes === bm.web.bytes,
    `disk=${webTree.files}/${webTree.bytes} manifest=${bm.web.files}/${bm.web.bytes}`);

  const dsDir = join(bundleRoot, "data", "europe-v5");
  const dsTree = await treeDigest(dsDir);
  check("dataset tree_sha256 与 bundle-manifest.json 一致（独立实现对同一算法的交叉核对）",
    dsTree.digest === bm.dataset.tree_sha256,
    `disk=${dsTree.digest.slice(0, 16)}... manifest=${String(bm.dataset.tree_sha256).slice(0, 16)}...`);
  check("dataset files/bytes 与 bundle-manifest.json 一致",
    dsTree.files === bm.dataset.files && dsTree.bytes === bm.dataset.bytes,
    `disk=${dsTree.files}/${dsTree.bytes} manifest=${bm.dataset.files}/${bm.dataset.bytes}`);
  check("dataset 目录含 manifest.json 且非空",
    existsSync(join(dsDir, "manifest.json")) && statSync(join(dsDir, "manifest.json")).size > 0);

  // 外层清单与 bundle 自述必须指向同一棵解压树
  console.log("\n-- 外层清单 vs 本解压树 --");
  check("release-manifest.json 的 artifact_sha256 指向本 ZIP",
    rel.artifact_sha256 === (await sha256File(ZIP)), `manifest=${String(rel.artifact_sha256).slice(0, 16)}...`);
  check("release-manifest.json 的 bundle_tree_sha256 与本解压树一致",
    rel.bundle_tree_sha256 === (await treeDigest(bundleRoot)).digest,
    `manifest=${String(rel.bundle_tree_sha256).slice(0, 16)}...`);

  void notVerified;
  console.log("");
  if (FAIL.length > 0) {
    console.log(`RELEASE ARTIFACT: FAIL (${FAIL.length}) - ${FAIL.join("; ")}`);
    console.log(`（通过 ${passCount} 项；未执行 ${notRun} 项）`);
    return 1;
  }
  console.log(`RELEASE ARTIFACT: PASS (${passCount} checks)`);
  return 0;
}

main()
  .then((code) => process.exit(code))
  .catch((e) => {
    console.error(`HARNESS FAILURE: ${e?.stack ?? e}`);
    process.exit(4);
  });
