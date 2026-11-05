// 验证：版本号正确显示 + 便携版检查更新链路可用。
import { chromium } from "playwright-core";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const ok = (n, c, extra = "") => {
  results.push([n, !!c]);
  console.log(`${c ? "PASS" : "FAIL"} | ${n}${extra ? " | " + extra : ""}`);
};

const browser = await chromium.connectOverCDP("http://127.0.0.1:9223");
let p = null;
for (let i = 0; i < 25 && !p; i++) {
  for (const cand of browser.contexts().flatMap((c) => c.pages())) {
    try {
      cand.setDefaultTimeout(15000);
      if ((await cand.locator("header").count()) > 0) { p = cand; break; }
    } catch {}
  }
  if (!p) await sleep(1000);
}
if (!p) { console.log("找不到主窗口"); process.exit(1); }

const invoke = (cmd, args) =>
  p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);

// ---------- 1. 版本号 ----------
const ver = await invoke("app_version");
console.log("app_version 返回:", ver);
ok("版本号不再是 0.1.0", ver !== "0.1.0", ver);
ok("版本号格式正确", /^\d+\.\d+\.\d+/.test(ver), ver);

// ---------- 2. 便携版检测 ----------
const info = await invoke("portable_info");
console.log("portable_info:", JSON.stringify(info));
ok("识别为便携版", info.isPortable === true);
ok("平台键格式正确", /^(windows|darwin|linux)-(x86_64|aarch64)$/.test(info.platformKey), info.platformKey);
ok("后端版本与 app_version 一致", info.version === ver, `${info.version} vs ${ver}`);

// ---------- 3. 后端拉取 latest.json（前端 fetch 会被 WebView 拦） ----------
let manifest = null;
try {
  manifest = await invoke("fetch_update_manifest", {
    url: "https://github.com/tocha688/FSProject-Manager/releases/latest/download/latest.json",
  });
} catch (e) {
  manifest = { error: String(e) };
}
ok("后端可拉取 latest.json", !manifest.error, manifest.error || "");
if (!manifest.error) {
  console.log("远端版本:", manifest.version, "| 便携包:", manifest.portableUrl.split("/").pop() || "(无)");
  ok("远端版本可解析", /^\d+\.\d+\.\d+/.test(manifest.version), manifest.version);
  ok("清单含当前平台的便携包", !!manifest.portableUrl, manifest.portableUrl.slice(-40));
}

// ---------- 4. UI 上的版本显示 ----------
await p.getByRole("button", { name: "设置", exact: true }).click();
await p.waitForSelector("text=软件更新", { timeout: 15000 });
await sleep(800);
const secText = await p.locator("section", { hasText: "软件更新" }).textContent();
ok("设置页显示正确版本", secText.includes(ver), secText.slice(0, 80));
ok("设置页标明便携版", secText.includes("便携版"), "");
ok("有检查更新按钮", (await p.locator("button", { hasText: "检查更新" }).count()) >= 1);

// ---------- 5. 点击检查更新 ----------
await p.locator("button", { hasText: "检查更新" }).first().click();
let outcome = "";
for (let i = 0; i < 40; i++) {
  await sleep(600);
  const t = await p.locator("section", { hasText: "软件更新" }).textContent();
  if (t.includes("已是最新版本")) { outcome = "uptodate"; break; }
  if (t.includes("下载并安装")) { outcome = "available"; break; }
  if (t.includes("检查失败")) { outcome = "error"; break; }
}
ok("检查更新有明确结果", outcome !== "", outcome);
ok("检查更新未报错", outcome !== "error", outcome);

const finalText = await p.locator("section", { hasText: "软件更新" }).textContent();
if (outcome === "error") console.log("错误详情:", finalText.slice(0, 300));
else console.log("结果:", finalText.replace(/\s+/g, " ").slice(0, 200));

await browser.close();
console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
process.exit(failed.length ? 1 : 0);
