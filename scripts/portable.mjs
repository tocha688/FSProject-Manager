#!/usr/bin/env node
// 生成便携版压缩包（免安装，解压即用）。
//
// 各平台产物：
//   Windows → ProjectFinder_<ver>_<arch>_portable.zip
//             （exe + 说明，数据库与配置跟随 exe 所在目录）
//   Linux   → ProjectFinder_<ver>_<arch>_portable.tar.gz（含 AppImage 与启动脚本）
//   macOS   → ProjectFinder_<ver>_<arch>_portable.tar.gz（.app 包）
//
// 用法：node scripts/portable.mjs
// 前置：已执行 npm run tauri build（或由 CI 在此之前构建）
import { execSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, copyFileSync, writeFileSync, rmSync, statSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const tauriConf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));

const version = tauriConf.version;
const productName = tauriConf.productName;
// 产物文件名统一用 ASCII 前缀：中文名在部分系统/CI 工具链里会引发编码问题，
// 而桌面快捷方式与窗口标题用的是 productName（可为中文）。
const FILE_PREFIX = "ProjectFinder";
const outDir = join(root, "src-tauri/target/release/bundle/portable");
const releaseDir = join(root, "src-tauri/target/release");

const platform = process.platform; // win32 | linux | darwin
const arch = process.arch; // x64 | arm64

// 命名里带上明确的操作系统标识，避免三平台的同名产物互相混淆
// （例如 Linux 与 Windows 都是 x64，只看架构无法区分）。
const osLabel = platform === "win32" ? "windows" : platform === "darwin" ? "macos" : "linux";
const archLabel = arch === "x64" ? "x64" : arch === "arm64" ? "aarch64" : arch;
const namePrefix = `${FILE_PREFIX}_${version}_${osLabel}_${archLabel}`;

function fail(msg) {
  console.error(`✗ ${msg}`);
  process.exit(1);
}

/** 断言构建产物存在，否则给出可操作的提示。 */
function requirePath(p, hint) {
  if (!existsSync(p)) fail(`找不到 ${p}\n  ${hint}`);
  return p;
}

function megabyte(p) {
  return `${(statSync(p).size / 1024 / 1024).toFixed(2)} MB`;
}

rmSync(outDir, { recursive: true, force: true });
mkdirSync(outDir, { recursive: true });

const staging = join(outDir, "staging");
mkdirSync(staging, { recursive: true });

const readme = `项目快搜 ${productName} v${version} — 便携版
========================================

免安装：解压到任意目录，双击主程序即可运行。

数据位置
--------
应用采用便携式存储：配置与数据库文件（config.json、projects.db）
会生成在主程序所在目录。把整个文件夹拷到别处即可迁移全部数据。

如果不希望数据放在程序目录（例如装在只读位置），
可设置环境变量指定数据目录：
  PF_DATA_DIR=<你的目录>

首次使用
--------
1. 打开「设置」，添加要扫描的项目根目录（如 ~/projects 或 D:\\Codes）
2. 配置 AI（可选）：填 API 地址与 Key，点「测试连接」通过后开启
3. 回到主界面点「扫描」
4. 按 Alt+Space 唤起快速搜索（可在设置里改）

更多说明见仓库 README。
`;

let artifact;

if (platform === "win32") {
  // 可执行文件名可能跟随 productName（中文）或保持 Cargo 包名，两种都找
  const candidates = [`${productName}.exe`, "project-mg-app.exe"];
  let exe = null;
  for (const c of candidates) {
    const p = join(releaseDir, c);
    if (existsSync(p)) { exe = p; break; }
  }
  if (!exe) {
    fail(`找不到可执行文件（尝试过 ${candidates.join(", ")}）\n  先运行 npm run tauri build`);
  }
  copyFileSync(exe, join(staging, `${FILE_PREFIX}.exe`));
  writeFileSync(join(staging, "使用说明.txt"), readme, "utf8");
  writeFileSync(join(staging, "portable.marker"), String(version), "utf8");

  artifact = join(outDir, `${namePrefix}_portable.zip`);
  execSync(
    `powershell -NoProfile -Command "Compress-Archive -Path '${staging}\\*' -DestinationPath '${artifact}' -Force"`,
    { stdio: "inherit" },
  );
} else if (platform === "darwin") {
  const appDir = requirePath(
    join(releaseDir, "bundle/macos", `${productName}.app`),
    "先运行 npm run tauri build 生成 .app",
  );
  execSync(`cp -R "${appDir}" "${staging}/"`, { stdio: "inherit" });
  writeFileSync(join(staging, "README.txt"), readme, "utf8");
  writeFileSync(join(staging, "portable.marker"), String(version), "utf8");

  artifact = join(outDir, `${namePrefix}_portable.tar.gz`);
  execSync(`tar -czf "${artifact}" -C "${staging}" .`, { stdio: "inherit" });
} else {
  // Linux：优先 AppImage（真正免安装），没有就回退裸二进制
  const appimageDir = join(releaseDir, "bundle/appimage");
  let appImage = null;
  if (existsSync(appimageDir)) {
    const found = readdirSync(appimageDir).find((f) => f.endsWith(".AppImage"));
    if (found) appImage = join(appimageDir, found);
  }
  if (appImage) {
    copyFileSync(appImage, join(staging, `${FILE_PREFIX}.AppImage`));
  } else {
    const bin = requirePath(
      join(releaseDir, "project-mg-app"),
      "先运行 npm run tauri build（Linux 需要 webkit2gtk 等系统依赖）",
    );
    copyFileSync(bin, join(staging, `${FILE_PREFIX}`));
  }
  writeFileSync(join(staging, "README.txt"), readme, "utf8");
  writeFileSync(join(staging, "portable.marker"), String(version), "utf8");
  // 启动脚本：自动加可执行权限
  writeFileSync(
    join(staging, "启动.sh"),
    `#!/bin/sh\ncd "$(dirname "$0")"\nchmod +x "${FILE_PREFIX}"* 2>/dev/null\n` +
      (appImage ? `exec "./${FILE_PREFIX}.AppImage" "$@"\n` : `exec "./${FILE_PREFIX}" "$@"\n`),
    { mode: 0o755 },
  );

  artifact = join(outDir, `${namePrefix}_portable.tar.gz`);
  execSync(`tar -czf "${artifact}" -C "${staging}" .`, { stdio: "inherit" });
}

rmSync(staging, { recursive: true, force: true });

if (!existsSync(artifact)) fail("打包未产出文件");
console.log(`✓ 便携版已生成：${artifact}（${megabyte(artifact)}）`);
