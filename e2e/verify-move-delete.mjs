// 验证项目「移动」与「删除」的处理策略。
// 全程使用隔离数据目录与临时夹具，不碰用户的真实项目库。
import { chromium } from "playwright-core";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const FIX = path.join(os.tmpdir(), "pf-move-test");
const DATA = path.join(FIX, "data");
const PROJ = path.join(FIX, "projects");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const ok = (n, c, extra = "") => {
  results.push([n, !!c]);
  console.log(`${c ? "PASS" : "FAIL"} | ${n}${extra ? " | " + extra : ""}`);
};

// ---------- 准备夹具 ----------
fs.rmSync(FIX, { recursive: true, force: true });
fs.mkdirSync(DATA, { recursive: true });
fs.mkdirSync(PROJ, { recursive: true });

function mkProject(dir, name, opts = {}) {
  const d = path.join(PROJ, dir);
  fs.mkdirSync(d, { recursive: true });
  fs.writeFileSync(path.join(d, "package.json"), JSON.stringify({ name, dependencies: opts.deps ?? {} }));
  if (opts.gitRemote) {
    // 造一个 .git/config 里的 remote，让采集能读到
    fs.mkdirSync(path.join(d, ".git"), { recursive: true });
    fs.writeFileSync(
      path.join(d, ".git", "config"),
      `[remote "origin"]\n\turl = ${opts.gitRemote}\n`,
    );
  }
  return d;
}

const moveSrc = mkProject("mover", "mover-app", {
  deps: { express: "1", lodash: "1", axios: "1", dotenv: "1" },
});
const delDir = mkProject("deleter", "deleter-app");
const remoteDir = mkProject("remote-proj", "remote-proj", {
  gitRemote: "https://github.com/example/remote-proj.git",
});

fs.writeFileSync(
  path.join(DATA, "config.json"),
  JSON.stringify(
    {
      roots: [{ path: PROJ, depth: 3 }],
      ai: {
        provider: "openai-compatible",
        baseUrl: "https://example.com/v1",
        apiKey: "",
        model: "m",
        enabled: false,
        concurrency: 2,
        verifiedFingerprint: "",
      },
      shortcut: "alt+space",
      editorCmd: "code",
      autostart: false,
      security: { passwordHash: "", idleLockMinutes: 1 },
    },
    null,
    2,
  ),
);

// ---------- 启动应用 ----------
const { spawn } = await import("node:child_process");
const exe = path.join(process.cwd(), "src-tauri", "target", "release", "project-mg-app.exe");
const child = spawn(exe, [], {
  env: {
    ...process.env,
    PF_DATA_DIR: DATA,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9224",
  },
  detached: true,
  stdio: "ignore",
});
child.unref();

await sleep(9000);

async function conn() {
  for (let i = 0; i < 30; i++) {
    try {
      const b = await chromium.connectOverCDP("http://127.0.0.1:9224");
      for (const p of b.contexts().flatMap((c) => c.pages())) {
        try {
          p.setDefaultTimeout(20000);
          if ((await p.locator("header").count()) > 0) return { b, p };
        } catch {}
      }
      await b.close();
    } catch {}
    await sleep(1000);
  }
  throw new Error("连不上应用");
}

const { b, p } = await conn();
const invoke = (cmd, args) =>
  p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);

// ---------- 1. 首次扫描 ----------
let scan = await invoke("scan_now");
console.log("首次扫描:", JSON.stringify(scan));
ok("首次扫描发现 3 个项目", scan.found === 3, String(scan.found));

let rows = await invoke("list_projects");
ok("库中 3 条记录", rows.length === 3, String(rows.length));

// 给 mover 写一条"用户数据"，用于验证移动后是否保留
const mover = rows.find((r) => r.path.endsWith("mover"));
await invoke("update_notes", { id: mover.id, notes: "移动测试的备注" });
await invoke("update_tags", { id: mover.id, tags: ["重要标签"] });
await invoke("set_password", { oldPassword: null, newPassword: "", idleLockMinutes: 1 }).catch(() => {});

// ---------- 2. 移动项目 ----------
const newDir = path.join(PROJ, "moved-here");
fs.renameSync(moveSrc, newDir);
console.log("已把 mover 从 projects/mover 移动到 projects/moved-here");

scan = await invoke("scan_now");
console.log("移动后扫描:", JSON.stringify(scan));
ok("识别出 1 个项目被移动", scan.moved === 1, String(scan.moved));

rows = await invoke("list_projects");
const movedRow = rows.find((r) => r.path.endsWith("moved-here"));
ok("新路径已入库", !!movedRow, movedRow?.path ?? "(无)");
ok("旧路径记录已不存在（被迁移而非重复）", !rows.some((r) => r.path.endsWith("projects\\mover")), "");
ok("用户备注保留", movedRow?.notes === "移动测试的备注", movedRow?.notes ?? "(空)");
ok("用户标签保留", JSON.stringify(movedRow?.tags) === '["重要标签"]', JSON.stringify(movedRow?.tags));
ok("总记录数未增加", rows.length === 3, String(rows.length));

// ---------- 3. 删除项目文件夹 ----------
fs.rmSync(delDir, { recursive: true, force: true });
console.log("已删除 projects/deleter");

scan = await invoke("scan_now");
console.log("删除后扫描:", JSON.stringify(scan));
ok("标记了 1 个已丢失", scan.markedMissing >= 1, String(scan.markedMissing));

rows = await invoke("list_projects");
const missingRow = rows.find((r) => r.path.endsWith("deleter"));
ok("记录仍保留（未删除）", !!missingRow, missingRow ? "存在" : "被删掉了");
ok("状态为 missing", missingRow?.status === "missing", missingRow?.status ?? "");
ok("总记录数仍是 3（未丢数据）", rows.length === 3, String(rows.length));

// ---------- 4. 把丢失的项目移回来 ----------
fs.mkdirSync(delDir, { recursive: true });
fs.writeFileSync(path.join(delDir, "package.json"), JSON.stringify({ name: "deleter-app" }));
scan = await invoke("scan_now");
console.log("移回后扫描:", JSON.stringify(scan));

rows = await invoke("list_projects");
const restored = rows.find((r) => r.path.endsWith("deleter"));
ok("移回后状态恢复（不再是 missing）", restored?.status !== "missing", restored?.status ?? "");
ok("最终仍是 3 条记录", rows.length === 3, String(rows.length));

// ---------- 5. 手动清理丢失记录 ----------
fs.rmSync(remoteDir, { recursive: true, force: true });
await invoke("scan_now");
let before = (await invoke("list_projects")).length;
const purged = await invoke("purge_missing_projects");
const after = (await invoke("list_projects")).length;
ok("清理已丢失记录生效", purged >= 1 && after === before - purged, `清理 ${purged}，${before}→${after}`);

console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));

await b.close();
try {
  process.kill(-child.pid);
} catch {}
try {
  fs.rmSync(FIX, { recursive: true, force: true, maxRetries: 5, retryDelay: 300 });
} catch {
  // 进程可能还占着目录，忽略即可
}
process.exit(failed.length ? 1 : 0);
