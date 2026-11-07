// 验证镜像测速与更新检查：确认会测速并选用可用镜像。
import { chromium } from "playwright-core";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const ok = (n, c, extra = "") => {
  results.push([n, !!c]);
  console.log(`${c ? "PASS" : "FAIL"} | ${n}${extra ? " | " + extra : ""}`);
};

const browser = await chromium.connectOverCDP("http://127.0.0.1:9226");
let p = null;
for (let i = 0; i < 30 && !p; i++) {
  for (const c of browser.contexts().flatMap((x) => x.pages())) {
    try {
      c.setDefaultTimeout(20000);
      if ((await c.locator("header").count()) > 0) { p = c; break; }
    } catch {}
  }
  if (!p) await sleep(1000);
}
if (!p) { console.log("找不到窗口"); process.exit(1); }

const invoke = (cmd, args) =>
  p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);

// ---------- 1. 镜像列表 ----------
const mirrors = await invoke("list_mirrors");
console.log("镜像列表:", mirrors.map((m) => `${m.label}${m.prefix ? "" : "(直连)"}`).join(", "));
ok("镜像列表非空", mirrors.length >= 2, String(mirrors.length));
ok("包含直连", mirrors.some((m) => m.prefix === ""));
ok("镜像都有 prefix 或为空", mirrors.every((m) => typeof m.prefix === "string"));

// ---------- 2. 测速 ----------
const LATEST = "https://github.com/tocha688/FSProject-Manager/releases/latest/download/latest.json";
console.log("开始测速（最多等 20 秒）…");
const t0 = Date.now();
let speeds = [];
try {
  speeds = await invoke("speed_test_mirrors", { probeUrl: LATEST });
} catch (e) {
  console.log("测速异常:", e);
}
const elapsed = Date.now() - t0;
console.log(`测速耗时 ${elapsed}ms`);
for (const s of speeds) {
  console.log(`  ${s.ok ? "✓" : "✗"} ${s.label.padEnd(16)} ${s.ok ? s.elapsedMs + "ms" : s.error.slice(0, 40)}`);
}
ok("测速返回结果", speeds.length > 0, String(speeds.length));
ok("测速在合理时间内完成（<25s）", elapsed < 25000, `${elapsed}ms`);

const usable = speeds.filter((s) => s.ok);
ok("至少有一个可用源", usable.length > 0, `${usable.length} 个`);
if (usable.length > 0) {
  const fastest = usable.sort((a, b) => a.elapsedMs - b.elapsedMs)[0];
  console.log(`最快: ${fastest.label} (${fastest.elapsedMs}ms)`);
  ok("最快源耗时合理", fastest.elapsedMs < 10000, `${fastest.elapsedMs}ms`);
}

// ---------- 3. 用镜像前缀拉清单 ----------
const best = usable[0];
if (best) {
  const prefix = mirrors.find((m) => m.id === best.id)?.prefix ?? "";
  console.log(`用 ${best.label} 拉取清单（prefix="${prefix}"）…`);
  try {
    const manifest = await invoke("fetch_update_manifest", {
      url: LATEST,
      mirrorPrefix: prefix,
    });
    ok("经镜像拉取清单成功", !!manifest.version, manifest.version);
    console.log("远端版本:", manifest.version, "| 便携包:", manifest.portableUrl.split("/").pop());
    ok("清单含便携包地址", !!manifest.portableUrl, manifest.portableUrl.slice(0, 60));
  } catch (e) {
    ok("经镜像拉取清单成功", false, String(e).slice(0, 80));
  }
}

// ---------- 4. UI 检查更新 ----------
await p.getByRole("button", { name: "设置", exact: true }).click();
await p.waitForSelector("text=软件更新", { timeout: 15000 });
await sleep(600);
const t1 = Date.now();
await p.locator("button", { hasText: "检查更新" }).first().click();
let outcome = "";
for (let i = 0; i < 60; i++) {
  await sleep(700);
  const txt = await p.locator("section", { hasText: "软件更新" }).textContent();
  if (txt.includes("已是最新版本")) { outcome = "uptodate"; break; }
  if (txt.includes("下载并安装")) { outcome = "available"; break; }
  if (txt.includes("检查失败")) { outcome = "error"; break; }
}
const uiElapsed = Date.now() - t1;
console.log(`UI 检查更新耗时 ${uiElapsed}ms，结果: ${outcome || "超时"}`);
ok("UI 检查更新有结果", outcome !== "", outcome);
ok("UI 检查更新未失败", outcome !== "error", outcome);

await browser.close();
console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
process.exit(failed.length ? 1 : 0);
