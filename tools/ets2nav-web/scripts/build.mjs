#!/usr/bin/env node
// ETS2Nav 正式 Web UI 静态产物构建（P4R-01 前端可复现性）
//
// 目标：clean clone → `npm ci` → `npm run build` → 生成 dist/ 下明确的静态产物，
// 不需要人工复制任何未记录文件，也不依赖运行时公共 CDN。
//
// 依赖治理规则：
//   1. 版本由 package.json + package-lock.json 唯一确定（无 ^ ~ 范围）；
//   2. upstream 已提供浏览器 bundle 的库直接复制其官方 dist 产物（maplibre-gl、pmtiles），
//      不重新打包，避免二次构建引入版本漂移；
//   3. upstream 未提供浏览器 bundle 的库（qrcode）才由 esbuild 打包为 IIFE；
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

  // 2) upstream 浏览器 bundle：MapLibre GL JS（脚本 + 样式）与 PMTiles
  await copyVendorBundle("maplibre-gl", "dist/maplibre-gl.js", "maplibre-gl.js", [
    "addProtocol",
  ]);
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
