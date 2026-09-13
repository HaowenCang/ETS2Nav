#!/usr/bin/env node
// ETS2Nav 正式 Web UI 静态产物构建（P4R-01 前端可复现性）
//
// 目标：clean clone → `npm ci` → `npm run build` → 生成 dist/ 下明确的静态产物，
// 不需要人工复制任何未记录文件，也不依赖运行时公共 CDN。
//
// 依赖治理规则：
//   1. 版本由 package.json + package-lock.json 唯一确定（无 ^ ~ 范围）；
//   2. upstream 已提供浏览器 bundle 的库直接复制其官方 dist 产物（pmtiles），
//      不重新打包，避免二次构建引入版本漂移；
//   3. upstream 未提供浏览器 bundle 的库由 esbuild 打包——现有两例原因不同：
//      qrcode 只发布 CommonJS；maplibre-gl 自 v6 起只发布 ESM（UMD / CSP /
//      CommonJS 构建均已移除），见 bundleMaplibre()；
//   4. 产物哈希、版本、license 写入 dist/build-manifest.json，供 CI 与 clean-clone 校验。
//
// 入参：无。可选资源（map.pmtiles、fonts/）缺失不构成构建失败——它们是运行期资源，
//       由 map-compiler（P1-12）产生，不入库。

import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { build as esbuild } from "esbuild";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const NODE_MODULES = join(ROOT, "node_modules");
const DIST = join(ROOT, "dist");
const DIST_VENDOR = join(DIST, "vendor");

/** 直接复制的源静态文件（属于仓库内容，缺失即构建失败）。 */
const STATIC_FILES = ["index.html", "app.js", "style.css", "manifest.json"];

/** 运行期可选资源（不入库）：缺失仅告警，不失败。 */
const OPTIONAL_ASSETS = ["map.pmtiles"];

function fail(msg) {
  console.error(`[build] 失败：${msg}`);
  process.exit(1);
}

async function sha256(path) {
  return createHash("sha256").update(await readFile(path)).digest("hex");
}

/** 读取已安装包的真实版本 / license / 来源，作为依赖记录写入 manifest。 */
async function pkgInfo(name) {
  const p = join(NODE_MODULES, name, "package.json");
  if (!existsSync(p)) fail(`依赖 ${name} 未安装（请先执行 npm ci）`);
  const meta = JSON.parse(await readFile(p, "utf8"));
  let repository = meta.repository;
  if (repository && typeof repository === "object") repository = repository.url;
  return {
    version: meta.version,
    license: meta.license ?? null,
    repository: repository ?? meta.homepage ?? null,
  };
}

/**
 * 复制 upstream 官方浏览器 bundle。
 * 复制后按必需符号做一次集成断言——upstream 改变打包形态时立即失败，
 * 而不是把问题留到浏览器运行期。
 */
async function copyVendorBundle(pkg, fromRel, toName, requiredSymbols) {
  const src = join(NODE_MODULES, pkg, fromRel);
  if (!existsSync(src)) {
    fail(`${pkg} 未提供预期产物 ${fromRel}（upstream 打包形态可能已变化）`);
  }
  await cp(src, join(DIST_VENDOR, toName));
  const text = await readFile(src, "utf8");
  for (const sym of requiredSymbols) {
    if (!text.includes(sym)) {
      fail(`${pkg}/${fromRel} 缺少必需符号 ${sym}——接入前置条件不成立`);
    }
  }
}

/** MapLibre 的 ESM 入口（v6 起 dist/ 下只有 .mjs）与需要产出的静态文件名。 */
const MAPLIBRE_MAIN_ENTRY = "dist/maplibre-gl.mjs";
const MAPLIBRE_WORKER_ENTRY = "dist/maplibre-gl-worker.mjs";
const MAPLIBRE_WORKER_FILE = "maplibre-gl-worker.js";

/**
 * 打包 MapLibre GL JS（主库 + worker）。
 *
 * 为什么不能像 pmtiles 那样「直接复制官方浏览器产物」：6.x 移除了 UMD / CSP /
 * CommonJS 三种构建，dist/ 下只剩 maplibre-gl.mjs、maplibre-gl-shared.mjs 与
 * maplibre-gl-worker.mjs，且 package.json 只有 `import` 导出条件（`require` 以
 * ERR_PACKAGE_PATH_NOT_EXPORTED 失败）。index.html 里那个
 * `dist/maplibre-gl.js` 在 6.x 中根本不存在。
 *
 * 两种产出形态都是被接入约束逼出来的，不是风格偏好：
 *   · 主库 → IIFE + 全局 `maplibregl`：index.html 以经典脚本加载它，app.js 也是
 *     经典脚本并直接引用全局名。改成 `type="module"` 会改变整页脚本的加载与执行
 *     时序（延迟到解析完成），与本页 DOM 已就绪的既有假设冲突。
 *   · worker → 独立静态文件：MapLibre 以 `new Worker(url, {type:'module'})` 自行
 *     加载 worker，其默认地址由模块自身的 `import.meta.url` 推导；打包成 IIFE 后
 *     `import.meta.url` 不再指向库文件，库推导失败并退化为空串（源码中
 *     `getWorkerUrl()` 在 import.meta.url 非 http(s) 时返回 ``），`new Worker("")`
 *     必然失败、地图永远不进入 loaded。因此必须由应用显式 setWorkerUrl 指向本产物。
 *     文件名后缀取 `.js` 而非 `.mjs`：nav-server 的 MIME 表只把 `.js` 映射为
 *     text/javascript，`.mjs` 落到 application/octet-stream，配合 nosniff 会被
 *     浏览器按「模块脚本 MIME 严格校验」拒绝。
 */
async function bundleMaplibre() {
  const mainEntry = join(NODE_MODULES, "maplibre-gl", MAPLIBRE_MAIN_ENTRY);
  const workerEntry = join(NODE_MODULES, "maplibre-gl", MAPLIBRE_WORKER_ENTRY);
  for (const p of [mainEntry, workerEntry]) {
    if (!existsSync(p)) {
      fail(`maplibre-gl 未提供 ${p}——upstream 打包形态可能已再次变化`);
    }
  }

  // 主库：ESM → IIFE（全局 maplibregl）
  await esbuild({
    entryPoints: [mainEntry],
    outfile: join(DIST_VENDOR, "maplibre-gl.js"),
    bundle: true,
    format: "iife",
    globalName: "maplibregl",
    platform: "browser",
    target: ["es2020"],
    legalComments: "inline",
    logLevel: "warning",
  });

  // worker：ESM → 单文件 ESM（依赖在打包时内联，运行期不得再有相对 import）
  await esbuild({
    entryPoints: [workerEntry],
    outfile: join(DIST_VENDOR, MAPLIBRE_WORKER_FILE),
    bundle: true,
    format: "esm",
    platform: "browser",
    target: ["es2020"],
    legalComments: "inline",
    logLevel: "warning",
  });

  // 集成断言：接入所依赖的公开符号必须在主库产物中真实存在。
  // setWorkerUrl 是 v6 的硬性接入点（见上），缺失即意味着应用会创建不出地图。
  const libText = await readFile(join(DIST_VENDOR, "maplibre-gl.js"), "utf8");
  for (const sym of ["addProtocol", "setWorkerUrl", "getWorkerUrl", "LngLatBounds"]) {
    if (!libText.includes(sym)) {
      fail(`maplibre-gl.js 缺少必需符号 ${sym}——v6 接入前置条件不成立`);
    }
  }
  if (!libText.includes("maplibregl")) {
    fail("maplibre-gl.js 未导出全局名 maplibregl");
  }

  // worker 自包含断言：若 esbuild 未内联 shared 分块，运行期会去请求一个不存在的
  // 相对路径，失败点会推迟到浏览器里且信息含糊，故在此提前失败。
  const workerText = await readFile(join(DIST_VENDOR, MAPLIBRE_WORKER_FILE), "utf8");
  if (/(?:^|\n)\s*import[\s\S]{0,400}?from\s*["']\.\//.test(workerText)) {
    fail(`${MAPLIBRE_WORKER_FILE} 仍含相对 import——worker 未被打成自包含文件`);
  }
  if (!workerText.includes("maplibre")) {
    fail(`${MAPLIBRE_WORKER_FILE} 内容可疑（不含 maplibre 标识）`);
  }

  // 应用侧接线断言：worker 地址只能由应用显式给出，漏掉即是运行期硬失败。
  const appText = await readFile(join(ROOT, "app.js"), "utf8");
  if (!appText.includes("maplibregl.setWorkerUrl(")) {
    fail(`app.js 未调用 maplibregl.setWorkerUrl()——MapLibre 6.x 打包后无法自行定位 worker`);
  }
  if (!appText.includes(MAPLIBRE_WORKER_FILE)) {
    fail(`app.js 的 setWorkerUrl 未指向 ${MAPLIBRE_WORKER_FILE}`);
  }
}

async function main() {
  if (!existsSync(NODE_MODULES)) {
    fail("node_modules 不存在。请先在 tools/ets2nav-web 执行 `npm ci`（或 `npm install`）。");
  }

  const deps = {
    "maplibre-gl": await pkgInfo("maplibre-gl"),
    pmtiles: await pkgInfo("pmtiles"),
    qrcode: await pkgInfo("qrcode"),
  };
  const devDeps = { esbuild: await pkgInfo("esbuild") };

  await rm(DIST, { recursive: true, force: true });
  await mkdir(DIST_VENDOR, { recursive: true });

  // 1) 源静态文件
  for (const name of STATIC_FILES) {
    const src = join(ROOT, name);
    if (!existsSync(src)) fail(`缺少源文件 ${name}`);
    await cp(src, join(DIST, name));
  }

  // 2) MapLibre GL JS：自 v6 起只发布 ESM，不再是「复制官方浏览器产物」而是打包
  await bundleMaplibre();
  await copyVendorBundle("maplibre-gl", "dist/maplibre-gl.css", "maplibre-gl.css", [
    ".maplibregl-map",
  ]);
  await copyVendorBundle("pmtiles", "dist/pmtiles.js", "pmtiles.js", ["Protocol"]);

  // 3) qrcode：upstream 只提供 CommonJS，需打包为浏览器 IIFE（全局 QRCode）
  await esbuild({
    entryPoints: [join(NODE_MODULES, "qrcode", "lib", "browser.js")],
    outfile: join(DIST_VENDOR, "qrcode.min.js"),
    bundle: true,
    format: "iife",
    globalName: "QRCode",
    platform: "browser",
    target: ["es2020"],
    minify: true,
    legalComments: "none",
    logLevel: "warning",
  });
  const qrText = await readFile(join(DIST_VENDOR, "qrcode.min.js"), "utf8");
  if (!qrText.includes("toCanvas")) fail("qrcode 打包产物缺少 toCanvas（API 变更？）");

  // 4) 运行期可选资源
  const optional = [];
  for (const name of OPTIONAL_ASSETS) {
    const src = join(ROOT, name);
    if (existsSync(src)) {
      await cp(src, join(DIST, name));
      optional.push(name);
    } else {
      console.warn(`[build] 提示：未提供 ${name}——运行期进入无底图模式（route/vehicle 照常）`);
    }
  }
  const fontsSrc = join(ROOT, "fonts");
  if (existsSync(fontsSrc)) {
    await cp(fontsSrc, join(DIST_VENDOR, "fonts"), { recursive: true });
    optional.push("fonts/");
  } else {
    console.warn("[build] 提示：未提供 fonts/——运行期跳过 city 文字层（几何图层照常）");
  }

  // 5) 产物记录（不含时间戳，保证同输入下产物与 manifest 可字节级复现）
  const artifacts = {};
  for (const rel of [
    "index.html",
    "app.js",
    "style.css",
    "manifest.json",
    "vendor/maplibre-gl.js",
    "vendor/maplibre-gl.css",
    `vendor/${MAPLIBRE_WORKER_FILE}`,
    "vendor/pmtiles.js",
    "vendor/qrcode.min.js",
  ]) {
    const abs = join(DIST, rel);
    artifacts[rel] = {
      sha256: await sha256(abs),
      bytes: (await readFile(abs)).length,
    };
  }

  const manifest = {
    generator: "tools/ets2nav-web/scripts/build.mjs",
    dependencies: deps,
    devDependencies: devDeps,
    artifacts,
    optionalAssets: optional,
  };
  await writeFile(
    join(DIST, "build-manifest.json"),
    `${JSON.stringify(manifest, null, 2)}\n`,
    "utf8",
  );

  console.log("[build] dist/ 生成完成");
  for (const [rel, info] of Object.entries(artifacts)) {
    console.log(`  ${rel.padEnd(28)} ${String(info.bytes).padStart(8)} B  ${info.sha256.slice(0, 16)}`);
  }
  console.log(`  可选资源: ${optional.length ? optional.join(", ") : "（无）"}`);
  console.log(`  输出目录: ${relative(process.cwd(), DIST) || DIST}`);
}

await main();
