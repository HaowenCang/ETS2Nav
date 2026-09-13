#!/usr/bin/env node
// graph-debugger 浏览器依赖构建（P4R Batch 6B §graph-debugger 去运行时 CDN）。
//
// 背景：本页原先以经典脚本从公共 CDN 加载 maplibre-gl@4.7.1。该版本落在
// GHSA-jrc7-96c5-q579 / CVE-2026-85061 的影响区间内（DOM.sanitize() 的 XSS 净化器
// 绕过，critical，CVSS 3.1 = 10.0，影响 <= 6.4.0，首个修复版本 6.4.1）。正式 Web UI
// 已在 P4R Batch 6A 升到 6.4.1，调试页此前未跟进。本脚本把调试页的浏览器依赖固化为
// 同源静态产物，运行期不再访问任何公共 CDN。
//
// 依赖来源：复用 tools/ets2nav-web 的 node_modules 与其 package.json 钉版，不新建 npm
// 工程。同一依赖存在两份副本时版本门会失去唯一判据，且两份副本必然各自漂移，故此处
// 只做打包，不做安装。
//
// 产物写入本目录 vendor/（构建输出，与 tools/ets2nav-web/dist 同属 .gitignore 管辖）：
//   maplibre-gl.js         MapLibre 主库：ESM → IIFE，全局名 maplibregl
//   maplibre-gl-worker.js  MapLibre worker：ESM → 自包含 ESM
//   maplibre-gl.css        upstream 官方产物，直接复制
//   pmtiles.js             upstream 官方浏览器产物，直接复制（UMD，全局 pmtiles）
//   fonts/（可选）          本地 glyphs；缺失仅告警，页面跳过 city 文字层
//   build-manifest.json    依赖版本与产物哈希，供 verify-graph-debugger.mjs 比对
//
// 为什么主库必须打成 IIFE、worker 必须另存为 .js（与 tools/ets2nav-web/scripts/build.mjs
// 同一约束，原因不是风格偏好）：
//   · MapLibre 自 v6 起只发布 ESM（dist/ 下只有 .mjs，package.json 无 require 条件），
//     而本页是经典脚本页面，改 type="module" 会改变整页脚本的加载与执行时序；
//   · worker 由库以模块 worker（new Worker(url, {type:"module"})）加载，其默认地址由
//     库自身的 import.meta.url 推导；打包成 IIFE 后该值不再指向库文件，库推导失败并
//     退化为空串，必须由页面显式 setWorkerUrl 指向本产物；
//   · worker 文件名后缀取 .js 而非 .mjs：服务端 MIME 表（nav-core nav-server 的
//     content_type）只把 .js 映射为 text/javascript，.mjs 落到 application/octet-stream，
//     配合 nosniff 会被浏览器按「模块脚本 MIME 严格校验」拒绝。

import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url))); // tools/graph-debugger
const WEB = resolve(ROOT, "..", "ets2nav-web"); // tools/ets2nav-web（依赖唯一来源）
const NODE_MODULES = join(WEB, "node_modules");
const VENDOR = join(ROOT, "vendor");

/** 安全下限自述常量。权威判据在 tools/ets2nav-web/scripts/verify-maplibre-version.mjs，
 *  此处只断言「调试页产物不低于同一下限」，不访问网络、不读 advisory 数据库。 */
const ADVISORY_FLOOR = "6.4.1";

/** MapLibre 的 ESM 入口（v6 起 dist/ 下只有 .mjs）与需要产出的静态文件名。 */
const MAPLIBRE_MAIN_ENTRY = "dist/maplibre-gl.mjs";
const MAPLIBRE_WORKER_ENTRY = "dist/maplibre-gl-worker.mjs";
const MAPLIBRE_WORKER_FILE = "maplibre-gl-worker.js";

/** 页面必须引用的本地产物，以及必须出现的接线调用。 */
const HTML_REQUIRED_REFS = [
  "vendor/maplibre-gl.css",
  "vendor/maplibre-gl.js",
  "vendor/pmtiles.js",
  "vendor/maplibre-gl-worker.js",
];

/** 运行期可选资源：属其它工具产出的资产，缺失仅告警。 */
const OPTIONAL_VENDOR_DIRS = [
  { from: join(WEB, "fonts"), to: "fonts", label: "fonts/（glyphs）" },
];

function fail(msg) {
  console.error(`[graph-debugger:build] 失败：${msg}`);
  process.exit(1);
}

function note(msg) {
  console.log(`[graph-debugger:build] ${msg}`);
}

async function sha256(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

/** 读取已安装包的真实版本 / license / 来源，作为依赖记录写入 manifest。 */
async function pkgInfo(name) {
  const p = join(NODE_MODULES, name, "package.json");
  if (!existsSync(p)) fail(`依赖 ${name} 未安装（请先在 tools/ets2nav-web 执行 npm ci）`);
  const meta = JSON.parse(await readFile(p, "utf8"));
  let repository = meta.repository;
  if (repository && typeof repository === "object") repository = repository.url;
  return {
    version: meta.version,
    license: meta.license ?? null,
    repository: repository ?? meta.homepage ?? null,
  };
}

/** 仅用于精确钉版的三段式比较；prerelease 不参与（本项目钉的都是正式版）。 */
function compareVersion(a, b) {
  const pa = String(a).split(".").map(Number);
  const pb = String(b).split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x !== y) return x < y ? -1 : 1;
  }
  return 0;
}

/**
 * 从 tools/ets2nav-web 解析 esbuild。本目录没有自己的 node_modules，故以该包的
 * package.json 为 require 根解析，而不是把 esbuild 复制一份过来。
 */
function loadEsbuild() {
  const requireFromWeb = createRequire(join(WEB, "package.json"));
  try {
    return requireFromWeb("esbuild");
  } catch (e) {
    fail(`无法从 ${NODE_MODULES} 加载 esbuild：${e.message}（请先执行 npm ci）`);
  }
}

/**
 * 复制 upstream 官方浏览器产物。
 * 复制后按必需符号做一次集成断言——upstream 改变打包形态时立即失败，
 * 而不是把问题留到浏览器运行期。
 */
async function copyVendorBundle(pkg, fromRel, toName, requiredSymbols) {
  const src = join(NODE_MODULES, pkg, fromRel);
  if (!existsSync(src)) fail(`${pkg} 未提供预期产物 ${fromRel}（upstream 打包形态可能已变化）`);
  await cp(src, join(VENDOR, toName));
  const text = await readFile(src, "utf8");
  for (const sym of requiredSymbols) {
    if (!text.includes(sym)) fail(`${pkg}/${fromRel} 缺少必需符号 ${sym}——接入前置条件不成立`);
  }
}

/** 打包 MapLibre GL JS（主库 + worker），并做接入前置条件断言。 */
async function bundleMaplibre(esbuild, expectVersion) {
  const mainEntry = join(NODE_MODULES, "maplibre-gl", MAPLIBRE_MAIN_ENTRY);
  const workerEntry = join(NODE_MODULES, "maplibre-gl", MAPLIBRE_WORKER_ENTRY);
  for (const p of [mainEntry, workerEntry]) {
    if (!existsSync(p)) fail(`maplibre-gl 未提供 ${p}——upstream 打包形态可能已再次变化`);
  }

  // 主库：ESM → IIFE（全局 maplibregl）
  await esbuild.build({
    entryPoints: [mainEntry],
    outfile: join(VENDOR, "maplibre-gl.js"),
    bundle: true,
    format: "iife",
    globalName: "maplibregl",
    platform: "browser",
    target: ["es2020"],
    legalComments: "inline",
    logLevel: "warning",
  });

  // worker：ESM → 单文件 ESM（依赖在打包时内联，运行期不得再有相对 import）
  await esbuild.build({
    entryPoints: [workerEntry],
    outfile: join(VENDOR, MAPLIBRE_WORKER_FILE),
    bundle: true,
    format: "esm",
    platform: "browser",
    target: ["es2020"],
    legalComments: "inline",
    logLevel: "warning",
  });

  // 集成断言：接入所依赖的公开符号必须在主库产物中真实存在。
  // setWorkerUrl 是 v6 的硬性接入点，getVersion 是运行期版本自证的唯一入口。
  const libText = await readFile(join(VENDOR, "maplibre-gl.js"), "utf8");
  for (const sym of ["addProtocol", "setWorkerUrl", "getWorkerUrl", "getVersion", "LngLatBounds"]) {
    if (!libText.includes(sym)) fail(`maplibre-gl.js 缺少必需符号 ${sym}——v6 接入前置条件不成立`);
  }
  if (!libText.includes("maplibregl")) fail("maplibre-gl.js 未导出全局名 maplibregl");
  if (!libText.includes(expectVersion)) {
    fail(`maplibre-gl.js 产物中未出现版本字符串 ${expectVersion}——产物与钉版可能不一致`);
  }

  // worker 自包含断言：若 esbuild 未内联 shared 分块，运行期会去请求一个不存在的
  // 相对路径，失败点会推迟到浏览器里且信息含糊，故在此提前失败。
  const workerText = await readFile(join(VENDOR, MAPLIBRE_WORKER_FILE), "utf8");
  if (/(?:^|\n)\s*import[\s\S]{0,400}?from\s*["']\.\//.test(workerText)) {
    fail(`${MAPLIBRE_WORKER_FILE} 仍含相对 import——worker 未被打成自包含文件`);
  }
  if (!workerText.includes("maplibre")) fail(`${MAPLIBRE_WORKER_FILE} 内容可疑（不含 maplibre 标识）`);
}

/**
 * 应用侧接线断言：页面只能引用本地产物，且必须显式 setWorkerUrl。
 *
 * 其中「不得出现 http(s) 形式的 src/href」是本页去 CDN 的构建期判据——它比运行期
 * 观测更早失败，且不依赖网络是否可达（断网时 CDN 请求只会表现为加载失败，容易被
 * 误读成「没问题」）。
 */
async function assertPageWiring() {
  const htmlPath = join(ROOT, "index.html");
  if (!existsSync(htmlPath)) fail("缺少 index.html");
  const html = await readFile(htmlPath, "utf8");

  for (const ref of HTML_REQUIRED_REFS) {
    if (!html.includes(ref)) fail(`index.html 未引用 ${ref}`);
  }
  const remote = /(?:src|href)\s*=\s*["']https?:\/\//i.exec(html);
  if (remote) {
    fail(`index.html 仍引用外部地址（${remote[0]}）——运行期不得依赖公共 CDN`);
  }
  if (!html.includes("maplibregl.setWorkerUrl(")) {
    fail("index.html 未调用 maplibregl.setWorkerUrl()——MapLibre 6.x 打包后无法自行定位 worker");
  }
}

async function main() {
  if (!existsSync(NODE_MODULES)) {
    fail("tools/ets2nav-web/node_modules 不存在。请先在该目录执行 npm ci（或 npm install）。");
  }

  // 依赖一致性：本例的 maplibre 只允许来自 tools/ets2nav-web 的钉版，且不得低于安全下限。
  const webPkg = JSON.parse(await readFile(join(WEB, "package.json"), "utf8"));
  const pin = webPkg?.dependencies?.["maplibre-gl"];
  if (typeof pin !== "string") fail("tools/ets2nav-web/package.json 未声明 maplibre-gl 依赖");
  const maplibre = await pkgInfo("maplibre-gl");
  if (maplibre.version !== pin) {
    fail(`已安装 maplibre-gl ${maplibre.version} 与钉版 ${pin} 不一致——请先执行 npm ci`);
  }
  if (compareVersion(maplibre.version, ADVISORY_FLOOR) < 0) {
    fail(
      `maplibre-gl ${maplibre.version} < 安全下限 ${ADVISORY_FLOOR}`
        + "（GHSA-jrc7-96c5-q579 / CVE-2026-85061）",
    );
  }

  const deps = { "maplibre-gl": maplibre, pmtiles: await pkgInfo("pmtiles") };
  const devDeps = { esbuild: await pkgInfo("esbuild") };

  await rm(VENDOR, { recursive: true, force: true });
  await mkdir(VENDOR, { recursive: true });

  const esbuild = loadEsbuild();
  await bundleMaplibre(esbuild, maplibre.version);
  await copyVendorBundle("maplibre-gl", "dist/maplibre-gl.css", "maplibre-gl.css", [".maplibregl-map"]);
  await copyVendorBundle("pmtiles", "dist/pmtiles.js", "pmtiles.js", ["Protocol"]);

  const optional = [];
  for (const { from, to, label } of OPTIONAL_VENDOR_DIRS) {
    if (existsSync(from)) {
      await cp(from, join(VENDOR, to), { recursive: true });
      optional.push(label);
    } else {
      console.warn(`[graph-debugger:build] 提示：未提供 ${label}——页面跳过 city 文字层（几何图层照常）`);
    }
  }

  await assertPageWiring();

  // 产物记录（不含时间戳，保证同输入下产物与 manifest 可字节级复现）。
  const artifacts = {};
  const artifactRels = [
    "vendor/maplibre-gl.js",
    "vendor/maplibre-gl.css",
    `vendor/${MAPLIBRE_WORKER_FILE}`,
    "vendor/pmtiles.js",
  ];
  for (const rel of artifactRels) {
    const abs = join(ROOT, rel);
    artifacts[rel] = { sha256: await sha256(abs), bytes: (await readFile(abs)).length };
  }

  const manifest = {
    generator: "tools/graph-debugger/build.mjs",
    advisoryFloor: ADVISORY_FLOOR,
    dependencies: deps,
    devDependencies: devDeps,
    artifacts,
    optionalAssets: optional,
  };
  await writeFile(join(VENDOR, "build-manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`, "utf8");

  note("vendor/ 生成完成");
  for (const [rel, info] of Object.entries(artifacts)) {
    console.log(`  ${rel.padEnd(30)} ${String(info.bytes).padStart(8)} B  ${info.sha256.slice(0, 16)}`);
  }
  console.log(`  可选资源: ${optional.length ? optional.join(", ") : "（无）"}`);
  console.log(`  输出目录: ${relative(process.cwd(), VENDOR) || VENDOR}`);
}

await main();
