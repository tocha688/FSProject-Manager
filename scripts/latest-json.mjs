#!/usr/bin/env node
// 生成 Tauri 自动更新所需的 latest.json 清单。
//
// Tauri 的 updater 会请求 endpoints 里配置的 URL，期望拿到形如：
//   { "version": "0.1.3",
//     "notes": "...",
//     "pub_date": "2026-11-05T00:00:00Z",
//     "platforms": {
//       "windows-x86_64": { "signature": "<.sig 内容>", "url": "<安装包直链>" },
//       "darwin-aarch64":  { ... },
//       "linux-x86_64":    { ... } } }
//
// 用法：node scripts/latest-json.mjs <release-files-dir> <repo> <tag>
// 依赖：同目录下存在 tauri build 产出的 *.sig 文件
import { readdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";

const [dir, repo, tag] = process.argv.slice(2);
if (!dir || !repo || !tag) {
  console.error("用法: node scripts/latest-json.mjs <dir> <owner/repo> <tag>");
  process.exit(1);
}

const version = tag.replace(/^v/, "");
const base = `https://github.com/${repo}/releases/download/${tag}`;

// 各平台优先使用的安装包（能自动更新的格式）：
//  Windows → NSIS 安装包；macOS → .app.tar.gz（Tauri 产出）；Linux → AppImage
const PREFERRED = {
  "windows-x86_64": [".exe"],
  "windows-aarch64": [".exe"],
  "darwin-x86_64": [".app.tar.gz"],
  "darwin-aarch64": [".app.tar.gz"],
  "linux-x86_64": [".AppImage"],
  "linux-aarch64": [".AppImage"],
};

const files = readdirSync(dir);
const platforms = {};

/**
 * macOS 的 .app.tar.gz 文件名里不带架构（Tauri 固定输出 ProjectFinder.app.tar.gz），
 * 无法从自身判断。改为看同批产物里的 .dmg 架构：
 *   ProjectFinder_0.1.4_aarch64.dmg → 整批是 arm64
 *   ProjectFinder_0.1.4_x64.dmg     → 整批是 x86_64
 * 同一 release 里 macOS 只会有一种架构（CI 单 runner 构建）。
 */
function macosArch() {
  const dmg = files.find((f) => f.endsWith(".dmg") && !f.endsWith(".sig"));
  if (!dmg) return null;
  return /aarch64|arm64/i.test(dmg) ? "aarch64" : "x86_64";
}

/** 在候选文件里挑出符合平台规则、且存在同名 .sig 的那个。 */
function pick(target) {
  const wanted = PREFERRED[target] ?? [];
  const arch = target.includes("aarch64") ? "aarch64" : "x86_64";
  const osPart = target.split("-")[0];

  // macOS 的 .app.tar.gz 自身无架构标识，用同批 dmg 推断；
  // 与推断结果不符的目标直接跳过，避免把 arm64 包装成 x86_64 的更新。
  if (osPart === "darwin") {
    const actual = macosArch();
    if (actual && actual !== arch) return null;
  }

  const candidates = files.filter((f) => {
    if (!wanted.some((ext) => f.endsWith(ext))) return false;
    if (f.endsWith(".sig")) return false;
    const lower = f.toLowerCase();
    // 便携版不用于自动更新（它是 zip/tar.gz 的另一种分发形态）
    if (lower.includes("portable")) return false;
    if (osPart === "windows" && !lower.endsWith(".exe")) return false;
    if (osPart === "linux" && !lower.endsWith(".appimage")) return false;
    if (osPart === "darwin") {
      // .app.tar.gz 文件名不含架构（已在上面用 dmg 校验过整批架构）
      return lower.endsWith(".app.tar.gz");
    }
    // Windows / Linux 的产物名带架构，做一次交叉校验
    if (arch === "aarch64" && !/aarch64|arm64/.test(lower)) return false;
    if (arch === "x86_64" && /aarch64|arm64/.test(lower)) return false;
    return true;
  });

  // 找有 .sig 的那个
  for (const c of candidates) {
    const sigPath = join(dir, `${c}.sig`);
    if (existsSync(sigPath)) {
      return { file: c, signature: readFileSync(sigPath, "utf8").trim() };
    }
  }
  return null;
}

for (const target of Object.keys(PREFERRED)) {
  const hit = pick(target);
  if (hit) {
    platforms[target] = {
      signature: hit.signature,
      url: `${base}/${encodeURIComponent(hit.file)}`,
    };
    console.log(`✓ ${target} → ${hit.file}`);
  }
}

if (Object.keys(platforms).length === 0) {
  console.error("✗ 没有找到任何带签名的更新包，latest.json 未生成");
  process.exit(1);
}

const manifest = {
  version,
  notes: `项目快搜 ${tag}`,
  pub_date: new Date().toISOString(),
  platforms,
  // 便携版更新用的额外字段（Tauri 内置 updater 会忽略它）：
  // 便携版自己读这个清单，从 portable 字段取 zip/tar.gz 直链。
  portable: {},
};

// 便携版产物：文件名形如 ProjectFinder_<ver>_<os>_<arch>_portable.zip
const OS_MAP = { windows: "windows", linux: "linux", macos: "darwin" };
for (const f of files) {
  if (f.endsWith(".sig")) continue;
  const m = f.match(/ProjectFinder_[\d.]+_(windows|linux|macos)_(x64|aarch64)_portable\.(zip|tar\.gz)$/);
  if (!m) continue;
  const target = `${OS_MAP[m[1]]}-${m[2] === "x64" ? "x86_64" : "aarch64"}`;
  manifest.portable[target] = { url: `${base}/${encodeURIComponent(f)}`, size: null };
  console.log(`✓ portable ${target} → ${f}`);
}

const out = join(dir, "latest.json");
writeFileSync(out, JSON.stringify(manifest, null, 2), "utf8");
console.log(
  `✓ 已生成 ${out}（安装版 ${Object.keys(platforms).length} 个平台，便携版 ${Object.keys(manifest.portable).length} 个平台）`,
);
