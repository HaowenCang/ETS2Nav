#!/usr/bin/env node
// MapLibre 版本门（P4R Batch 6A）——确定性、离线、只读本地文件。
//
// 目的：把「maplibre-gl 不低于已知安全下限，且三处版本声明互相一致」变成一条可
// 独立执行的判据，而不是嵌在别人输出里的一句结论。它不访问网络，也不读取
// advisory 数据库：安全下限以本文件中的常量自述，因此即便 GitHub Advisory API
// 不可达、区间描述被改写或本地 npm 缓存过期，本门给出的判定不变。
//
// ── 安全依据（自述常量）─────────────────────────────────────────────────────
//   GHSA-jrc7-96c5-q579  /  CVE-2026-85061
//   MapLibre GL JS：DOM.sanitize() 的 XSS 净化器绕过。缺陷机理是在移除属性的
//   同时迭代**活体** NamedNodeMap——每移除一项，集合实时收缩，紧随其后的下标
//   被跳过，于是部分属性（含事件处理器属性）得以保留。可达路径是 attribution
//   控件渲染不受信或第三方 attribution 字符串。
//   严重级别 critical（CVSS 3.1 = 10.0）；影响范围 <= 6.4.0；首个修复版本 6.4.1。
//   因此下限取 6.4.1。
const ADVISORY_FLOOR = "6.4.1";
const ADVISORY_ID = "GHSA-jrc7-96c5-q579";
const CVE_ID = "CVE-2026-85061";

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// ── semver 2.0.0（仅比较，不引入依赖）───────────────────────────────────────
// 官方 semver.org 正则：拒绝前导零、要求三段式、允许 prerelease / build。
const SEMVER_RE = new RegExp(
  "^(0|[1-9]\\d*)\\.(0|[1-9]\\d*)\\.(0|[1-9]\\d*)"
  + "(?:-((?:0|[1-9]\\d*|\\d*[a-zA-Z-][0-9a-zA-Z-]*)"
  + "(?:\\.(?:0|[1-9]\\d*|\\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?"
  + "(?:\\+([0-9a-zA-Z-]+(?:\\.[0-9a-zA-Z-]+)*))?$",
);

/** 解析 semver；不合法返回 null。 */
function parseSemver(v) {
  if (typeof v !== "string") return null;
  const m = SEMVER_RE.exec(v);
  if (!m) return null;
  return {
    raw: v,
    major: Number(m[1]),
    minor: Number(m[2]),
    patch: Number(m[3]),
    prerelease: m[4] ? m[4].split(".") : [],
  };
}

/** semver 2.0.0 §11 优先级比较：a < b 返回 -1，相等返回 0，a > b 返回 1。 */
function compareSemver(a, b) {
  for (const k of ["major", "minor", "patch"]) {
    if (a[k] !== b[k]) return a[k] < b[k] ? -1 : 1;
  }
  // 有 prerelease 的版本**低于**同号正式版。
  if (a.prerelease.length === 0 && b.prerelease.length === 0) return 0;
  if (a.prerelease.length === 0) return 1;
  if (b.prerelease.length === 0) return -1;
  const n = Math.max(a.prerelease.length, b.prerelease.length);
  for (let i = 0; i < n; i++) {
    const x = a.prerelease[i];
    const y = b.prerelease[i];
    if (x === undefined) return -1;          // 前缀更短者更小
    if (y === undefined) return 1;
    const xn = /^\d+$/.test(x);
    const yn = /^\d+$/.test(y);
    if (xn && yn) {
      const cx = Number(x);
      const cy = Number(y);
      if (cx !== cy) return cx < cy ? -1 : 1;
    } else if (xn !== yn) {
      return xn ? -1 : 1;                    // 数字标识符 < 字母数字标识符
    } else if (x !== y) {
      return x < y ? -1 : 1;
    }
  }
  return 0;
}

// ── 工具 ────────────────────────────────────────────────────────────────────
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/**
 * 读取并解析 JSON。
 *
 * 文件缺失或不可解析属于**本门自身的故障**（harness error），一律抛出：此时
 * 版本根本无从判定，若降级成 "FAIL: 版本过低" 就是在陈述一件未被验证的事，
 * 若降级成 PASS 则更糟。抛出使进程以非零退出且不打印 PASS 行。
 */
function readJson(rel) {
  const abs = join(ROOT, rel);
  let text;
  try {
    text = readFileSync(abs, "utf8");
  } catch (e) {
    throw new Error(`版本门无法读取 ${abs}：${e.message}（请先执行 npm ci）`);
  }
  try {
    return JSON.parse(text);
  } catch (e) {
    throw new Error(`版本门无法解析 ${abs}：${e.message}`);
  }
}

// ── 判定 ────────────────────────────────────────────────────────────────────
const violations = [];
const oks = [];

function check(ok, okLine, badLine) {
  if (ok) oks.push(okLine);
  else violations.push(badLine);
}

console.log("[gate] MapLibre 版本门（离线、只读本地文件）");
console.log(`[gate] 安全下限 ADVISORY_FLOOR = ${ADVISORY_FLOOR}`
  + `（${ADVISORY_ID} / ${CVE_ID}：critical，CVSS 3.1 = 10.0，影响 <= 6.4.0）`);

const floor = parseSemver(ADVISORY_FLOOR);
if (!floor) {
  throw new Error(`版本门自身参数非法：ADVISORY_FLOOR=${JSON.stringify(ADVISORY_FLOOR)} 不是合法 semver`);
}

// 条件 1：已安装版本合法且不低于安全下限
const installedRaw = readJson(join("node_modules", "maplibre-gl", "package.json")).version;
const installed = parseSemver(installedRaw);
check(
  installed !== null,
  `node_modules/maplibre-gl/package.json version = ${installedRaw}（合法 semver）`,
  `node_modules/maplibre-gl/package.json 的 version=${JSON.stringify(installedRaw)} 不是合法 semver`,
);
if (installed) {
  check(
    compareSemver(installed, floor) >= 0,
    `已安装版本 ${installedRaw} >= 安全下限 ${ADVISORY_FLOOR}`,
    `已安装 maplibre-gl ${installedRaw} < 安全下限 ${ADVISORY_FLOOR}`
      + `（受 ${ADVISORY_ID} / ${CVE_ID} 影响）`,
  );
}

// 条件 2：lockfile 钉住同一个版本
const lock = readJson("package-lock.json");
const lockPkg = lock?.packages?.["node_modules/maplibre-gl"];
if (!lockPkg || typeof lockPkg !== "object") {
  throw new Error('版本门无法判定：package-lock.json 中不存在 packages["node_modules/maplibre-gl"]');
}
const lockedRaw = lockPkg.version;
const locked = parseSemver(lockedRaw);
check(
  locked !== null,
  `package-lock.json packages["node_modules/maplibre-gl"].version = ${lockedRaw}（合法 semver）`,
  `package-lock.json 中 maplibre-gl 的 version=${JSON.stringify(lockedRaw)} 不是合法 semver`,
);
check(
  installed !== null && locked !== null && locked.raw === installed.raw,
  `lockfile 钉住版本与已安装版本一致（${lockedRaw}）`,
  `lockfile 版本 ${JSON.stringify(lockedRaw)} 与已安装版本 ${JSON.stringify(installedRaw)} 不一致`,
);
// lockfile 根依赖声明也应与 package.json 一致，否则「锁文件唯一确定版本」不成立
const lockRootSpec = lock?.packages?.[""]?.dependencies?.["maplibre-gl"];

// 条件 3：package.json 是精确钉版，且等于该版本
const pkg = readJson("package.json");
const spec = pkg?.dependencies?.["maplibre-gl"];
if (typeof spec !== "string") {
  throw new Error('版本门无法判定：package.json 的 dependencies 中没有 maplibre-gl');
}
// 精确钉版 = 合法 semver 且不含任何范围运算符 / 标签 / 通配。
const isExactPin = parseSemver(spec) !== null;
check(
  isExactPin,
  `package.json dependencies["maplibre-gl"] = "${spec}"（精确钉版，无 ^ ~ 范围）`,
  `package.json dependencies["maplibre-gl"] = ${JSON.stringify(spec)}`
    + " 不是精确钉版（禁止 ^ ~ 范围、latest、* 或任何 tag/URL 形式）",
);
check(
  installed !== null && isExactPin && spec === installed.raw,
  `package.json 钉版与已安装版本一致（${spec}）`,
  `package.json 钉版 ${JSON.stringify(spec)} 与已安装版本 ${JSON.stringify(installedRaw)} 不一致`,
);
check(
  lockRootSpec === spec,
  `package-lock.json 根依赖声明与 package.json 一致（"${lockRootSpec}"）`,
  `package-lock.json 根依赖声明 ${JSON.stringify(lockRootSpec)}`
    + ` 与 package.json 的 ${JSON.stringify(spec)} 不一致`,
);

// ── 输出 ────────────────────────────────────────────────────────────────────
for (const line of oks) console.log(`[gate] OK   ${line}`);
if (violations.length > 0) {
  for (const line of violations) console.error(`[gate] FAIL ${line}`);
  console.error(`[gate] 未通过：${violations.length} 项条件不满足`);
  process.exit(1);
}
console.log(`[gate] PASS maplibre-gl=${installedRaw}（>= 下限 ${ADVISORY_FLOOR}，`
  + `${ADVISORY_ID} / ${CVE_ID} 判定：不受影响）`);
process.exit(0);
