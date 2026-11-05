#!/usr/bin/env node
// 同步版本号到所有需要的地方。
//
// 背景（真实 bug）：版本号散落在三个文件里
//   package.json / src-tauri/tauri.conf.json / src-tauri/Cargo.toml
// 之前只改了前两个，Cargo.toml 一直停在 0.1.0，
// 而 Rust 的 app_version 读的正是 CARGO_PKG_VERSION ——
// 导致界面永远显示 0.1.0，进而让"检查更新"误判。
//
// 用法：
//   node scripts/version.mjs 0.1.7   # 设置版本
//   node scripts/version.mjs         # 只打印当前版本
import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const targets = [
  { path: "package.json", kind: "json" },
  { path: "src-tauri/tauri.conf.json", kind: "json" },
  { path: "src-tauri/Cargo.toml", kind: "cargo" },
];

const next = process.argv[2];

if (!next) {
  const versions = targets.map((t) => {
    const abs = join(root, t.path);
    if (t.kind === "json") return JSON.parse(readFileSync(abs, "utf8")).version;
    const m = readFileSync(abs, "utf8").match(/^version\s*=\s*"([^"]+)"/m);
    return m ? m[1] : "(未找到)";
  });
  targets.forEach((t, i) => console.log(`${t.path.padEnd(30)} ${versions[i]}`));
  const unique = new Set(versions);
  if (unique.size > 1) {
    console.error("\n✗ 版本号不一致！运行 node scripts/version.mjs <version> 同步");
    process.exit(1);
  }
  console.log("\n✓ 版本一致");
  process.exit(0);
}

if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(next)) {
  console.error(`✗ 版本号格式不合法: ${next}（应形如 0.1.7）`);
  process.exit(1);
}

for (const t of targets) {
  const abs = join(root, t.path);
  const raw = readFileSync(abs, "utf8");
  let out;
  if (t.kind === "json") {
    const j = JSON.parse(raw);
    j.version = next;
    out = JSON.stringify(j, null, 2) + "\n";
  } else {
    // 只替换 [package] 段里第一个 version
    out = raw.replace(/^version\s*=\s*"[^"]+"/m, `version = "${next}"`);
  }
  writeFileSync(abs, out);
  console.log(`✓ ${t.path} → ${next}`);
}
console.log(`\n下一步：`);
console.log(`  git commit -am "chore: 发布 v${next}"`);
console.log(`  git tag v${next} && git push origin main --tags`);
