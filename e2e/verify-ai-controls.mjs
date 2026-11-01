// 验证新增功能：AI 开关（测试通过才能开启）+ 并发 + 分析下拉菜单（未分析/全部重新）
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
const invoke = (cmd, args) => main.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);

// ---------- 初始状态 ----------
const before = await invoke("list_projects");
const beforePending = before.filter((p) => p.status === "pending" || p.status === "failed").length;
console.log(`初始：${before.length} 个项目，其中待分析 ${beforePending}`);

// ---------- 1. 设置页：开关默认禁用 ----------
await main.getByRole("button", { name: "设置", exact: true }).click();
await main.waitForSelector("text=AI 分析");
const sw = main.locator(".switch input[type=checkbox]");
ok("开关存在", (await sw.count()) === 1);
ok("未测试前开关被禁用", await sw.isDisabled());
const hint = await main.locator(".switch-hint").textContent();
ok("提示要求先测试连接", hint.includes("测试连接"), hint.trim());
ok("并发数输入存在", (await main.locator('input[type="number"]').count()) >= 2);

// ---------- 2. 测试连接 → 解锁开关 ----------
await main.getByRole("button", { name: "测试连接" }).click();
let tested = false;
for (let i = 0; i < 30; i++) {
  await sleep(1000);
  const t = (await main.locator(".toast").count()) ? await main.locator(".toast").textContent() : "";
  if (t.includes("连接成功")) { tested = true; break; }
  if (t.includes("失败") || t.includes("错误")) break;
}
ok("测试连接成功", tested);
await sleep(500);
const enabledNow = await sw.isEnabled();
ok("测试通过后开关解锁", enabledNow);

// ---------- 3. 启用并保存 ----------
if (enabledNow) {
  await sw.check();
  const conc = main.locator('input[type="number"]').last();
  await conc.fill("3");
  await main.getByRole("button", { name: "保存", exact: true }).click();
  await sleep(1500);
  const cfg = await invoke("get_settings");
  ok("AI 开关已持久化", cfg.ai.enabled === true);
  ok("并发数已保存为 3", cfg.ai.concurrency === 3, String(cfg.ai.concurrency));
  ok("后端确认已验证", cfg.aiVerified === true);
}

// ---------- 4. 分析下拉菜单 ----------
await main.getByRole("button", { name: "返回", exact: true }).click();
await main.waitForSelector("input.search");
const menuBtn = main.locator(".split-btn");
ok("分析下拉按钮存在", (await menuBtn.count()) === 1);
ok("按钮显示待分析计数徽标", (await main.locator(".count-dot").count()) === 1, await main.locator(".count-dot").textContent().catch(() => ""));

await menuBtn.click();
await main.waitForSelector(".menu");
const items = await main.locator(".menu .mi-title").allTextContents();
ok("菜单含「分析未分析项目」", items.some((t) => t.includes("分析未分析项目")), items.join(" / "));
ok("菜单含「全部重新分析」", items.some((t) => t.includes("全部重新分析")));

// ---------- 5. 执行「分析未分析项目」并等待真实完成 ----------
await main.locator(".menu button", { hasText: "分析未分析项目" }).click();
let toastOk = false;
for (let i = 0; i < 10; i++) {
  const t = (await main.locator(".toast").count()) ? await main.locator(".toast").textContent() : "";
  if (t.includes("已开始分析")) { toastOk = true; break; }
  await sleep(500);
}
ok("点击后有开始提示", toastOk);

let done = false;
let stats = null;
for (let i = 0; i < 120; i++) {
  await sleep(2000);
  const rows = await invoke("list_projects");
  const st = { pending: 0, analyzing: 0, ok: 0, failed: 0, candidate: 0 };
  for (const r of rows) st[r.status] = (st[r.status] || 0) + 1;
  stats = st;
  if (st.pending === 0 && st.analyzing === 0) { done = true; break; }
  if (i % 5 === 0) console.log(`  …进度 pending=${st.pending} analyzing=${st.analyzing} ok=${st.ok} failed=${st.failed}`);
}
ok("待分析项目全部跑完", done, JSON.stringify(stats));
ok("至少多数分析成功", stats && stats.ok >= Math.ceil((beforePending) * 0.7), `ok=${stats?.ok} failed=${stats?.failed} 目标≥${Math.ceil(beforePending * 0.7)}`);

const after = await invoke("list_projects");
const withDesc = after.filter((p) => p.description && p.description.trim()).length;
ok("AI 说明已写入数据库", withDesc > 0, `${withDesc}/${after.length} 条有说明`);
const sample = after.find((p) => p.description);
ok("说明内容非空且合理", sample && sample.description.length > 3, sample ? `${sample.name}: ${sample.description.slice(0, 40)}` : "");

// ---------- 6. 全部重新分析 ----------
await sleep(1000);
await menuBtn.click();
await main.waitForSelector(".menu");
await main.locator(".menu button", { hasText: "全部重新分析" }).click();
let allStarted = false;
for (let i = 0; i < 10; i++) {
  const t = (await main.locator(".toast").count()) ? await main.locator(".toast").textContent() : "";
  if (t.includes("全部重新分析")) { allStarted = true; break; }
  await sleep(500);
}
ok("「全部重新分析」已触发", allStarted);
let allDone = false;
for (let i = 0; i < 150; i++) {
  await sleep(2000);
  const rows = await invoke("list_projects");
  const pend = rows.filter((r) => r.status === "pending" || r.status === "analyzing").length;
  if (pend === 0) { allDone = true; break; }
}
ok("全部重新分析跑完", allDone);

// ---------- 7. 关闭开关后拒绝分析 ----------
const cfg2 = await invoke("get_settings");
ok("重新分析后开关仍为开启", cfg2.ai.enabled === true);
await invoke("save_settings", { s: { ...cfg2, ai: { ...cfg2.ai, enabled: false } } });
const afterDisable = await invoke("get_settings");
ok("可关闭开关", afterDisable.ai.enabled === false);
let rejected = false;
try {
  await invoke("analyze_pending", { mode: "pending" });
} catch (e) {
  rejected = String(e).includes("已关闭");
}
ok("关闭后分析请求被拒绝", rejected);

console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
await browser.close();
process.exit(failed.length ? 1 : 0);
