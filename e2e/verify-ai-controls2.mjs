// 补验：全部重新分析 + 关闭开关后拒绝分析。用 IPC 直连后端（避开 dev 热重载导致的页面重载）。
import { chromium } from "playwright-core";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const ok = (n, c, extra = "") => {
  results.push([n, !!c]);
  console.log(`${c ? "PASS" : "FAIL"} | ${n}${extra ? " | " + extra : ""}`);
};

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
let pages = browser.contexts().flatMap((c) => c.pages());
let main = null;
for (let i = 0; i < 40 && !main; i++) {
  for (const p of pages) {
    try {
      p.setDefaultTimeout(10000);
      if ((await p.locator("header").count()) > 0) { main = p; break; }
    } catch {}
  }
  if (!main) { await sleep(500); pages = browser.contexts().flatMap((c) => c.pages()); }
}
if (!main) { console.log("找不到主窗口"); process.exit(1); }

// 直接在后端上下文执行，不受页面重载影响
const out = await main.evaluate(async () => {
  const inv = window.__TAURI_INTERNALS__.invoke;
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const res = { steps: [] };
  const cfg0 = await inv("get_settings");
  res.startEnabled = cfg0.ai.enabled;
  res.startVerified = cfg0.aiVerified;

  // 1) 全部重新分析
  await inv("analyze_pending", { mode: "all" });
  res.steps.push("triggered-all");
  await sleep(1500);
  let sawWorking = false;
  let done = false;
  for (let i = 0; i < 180; i++) {
    const rows = await inv("list_projects");
    const active = rows.filter((r) => r.status === "pending" || r.status === "analyzing").length;
    if (active > 0) sawWorking = true;
    if (sawWorking && active === 0) { done = true; break; }
    await sleep(1000);
  }
  const rows2 = await inv("list_projects");
  res.allDone = done;
  res.sawWorking = sawWorking;
  res.statusAfterAll = rows2.reduce((m, r) => ((m[r.status] = (m[r.status] || 0) + 1), m), {});
  res.withDesc = rows2.filter((r) => r.description && r.description.trim()).length;
  res.total = rows2.length;

  // 2) 关闭开关 → 分析被拒绝
  const cfg = await inv("get_settings");
  await inv("save_settings", { s: { ...cfg, ai: { ...cfg.ai, enabled: false } } });
  res.afterDisable = (await inv("get_settings")).ai.enabled;
  try {
    await inv("analyze_pending", { mode: "pending" });
    res.rejected = false;
    res.rejectMsg = "";
  } catch (e) {
    res.rejected = true;
    res.rejectMsg = String(e);
  }

  // 3) 重新开启（恢复正常状态）
  const cfg2 = await inv("get_settings");
  await inv("save_settings", { s: { ...cfg2, ai: { ...cfg2.ai, enabled: true } } });
  res.restored = (await inv("get_settings")).ai.enabled;
  return res;
});

ok("初始 AI 开关为开启", out.startEnabled === true);
ok("初始配置已验证", out.startVerified === true);
ok("「全部重新分析」触发后项目重新进入队列", out.sawWorking);
ok("「全部重新分析」全部跑完", out.allDone, JSON.stringify(out.statusAfterAll));
ok("重新分析后仍有 AI 说明", out.withDesc > 0, `${out.withDesc}/${out.total}`);
ok("可关闭 AI 开关", out.afterDisable === false);
ok("关闭后分析请求被拒绝", out.rejected, out.rejectMsg.slice(0, 60));
ok("已恢复开关为开启", out.restored === true);

console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
await browser.close();
process.exit(failed.length ? 1 : 0);
