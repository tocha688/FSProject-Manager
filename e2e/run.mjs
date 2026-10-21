// E2E：通过 CDP 附着到真实运行的 Tauri 应用，逐项点击验证。
// 前置：应用已以 PF_DATA_DIR=<测试目录> + WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 启动。
import { chromium } from "playwright-core";
import fs from "node:fs";
import path from "node:path";

const VIZ =
  "C:\\Users\\admin\\.zcode\\v2\\visualizations\\b7186e246401ce3c154d398af0cf1b1bc95ff629de80c33740fb2736025ddff4";
fs.mkdirSync(VIZ, { recursive: true });

const results = [];
let shotN = 0;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function shot(page, name) {
  const p = path.join(VIZ, `${String(++shotN).padStart(2, "0")}-${name}.png`);
  try {
    await page.screenshot({ path: p });
    console.log(`  [shot] ${path.basename(p)}`);
  } catch (e) {
    console.log(`  [shot失败] ${name}: ${e.message}`);
  }
}

function ok(name, cond, extra = "") {
  results.push([name, !!cond]);
  console.log(`${cond ? "PASS" : "FAIL"} | ${name}${extra ? " | " + extra : ""}`);
}

// toast 3.5s 自动消失且可能叠加，必须等"包含指定文案"的 toast，而不是任意 .toast
async function waitToast(page, substr, timeout = 8000) {
  try {
    await page.waitForSelector(`.toast:has-text("${substr}")`, { timeout });
    return await page.locator(`.toast:has-text("${substr}")`).last().textContent();
  } catch {
    return null;
  }
}

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
let pages = browser.contexts().flatMap((c) => c.pages());
for (let i = 0; i < 20 && pages.length < 2; i++) {
  await sleep(500);
  pages = browser.contexts().flatMap((c) => c.pages());
}
console.log(`CDP 页面数: ${pages.length}`);

let main = null;
let quick = null;
for (let i = 0; i < 40 && !(main && quick); i++) {
  for (const p of pages) {
    if (main && quick) break;
    try {
      p.setDefaultTimeout(8000);
      if (!main && (await p.locator("header").count()) > 0) main = p;
      else if (!quick && (await p.locator(".quick").count()) > 0) quick = p;
    } catch {
      /* 页面未就绪，忽略 */
    }
  }
  if (!main || !quick) {
    await sleep(500);
    pages = browser.contexts().flatMap((c) => c.pages());
  }
}
ok("CDP 附着：主窗口", !!main);
ok("CDP 附着：快搜窗口", !!quick);
if (!main) process.exit(1);

// ---------- 1. 初始空状态 ----------
await main.waitForSelector("header", { timeout: 10000 });
ok("空状态提示", (await main.locator(".empty").count()) > 0);
await shot(main, "initial-empty");

// ---------- 2. 设置页 ----------
await main.getByRole("button", { name: "设置", exact: true }).click();
await main.waitForSelector("text=扫描目录");
const rootText = await main.locator("section .path").first().textContent();
ok("设置：根目录已预置", rootText.includes("pf-e2e-test"), rootText.trim());
const baseUrl = await main.locator('input[placeholder*="bigmodel"]').inputValue();
ok("设置：AI 地址已预置", baseUrl.includes("tmai.ak03.cn"), baseUrl);
await main.getByRole("button", { name: "保存", exact: true }).click();
ok("设置：保存 toast", (await waitToast(main, "设置已保存")) !== null);
await main.getByRole("button", { name: "返回", exact: true }).click();
await main.waitForSelector("input.search");
await shot(main, "settings");

// ---------- 3. 扫描 ----------
await main.getByRole("button", { name: "扫描", exact: true }).click();
const scanToast = await waitToast(main, "扫描完成", 25000);
ok("扫描完成 toast（发现 6 个）", scanToast !== null && scanToast.includes("发现 6 个"), scanToast ?? "未出现");
await main.waitForSelector(".card", { timeout: 10000 });
await sleep(400);
ok("扫描入库 6 个项目", (await main.locator(".card").count()) === 6);
ok("深度限制：too-deep 未入库", (await main.locator(".card", { hasText: "too-deep" }).count()) === 0);
ok("深度 3 嵌套项目已入库", (await main.locator(".card", { hasText: "nested-py" }).count()) >= 1);
const badgeTexts = await main.locator(".card .card-top .badge").nth(1).allTextContents
  ? await main.locator(".card .card-top .badge:nth-child(3)").allTextContents()
  : [];
const candN = badgeTexts.filter((t) => t.includes("待确认")).length;
ok("状态：1 待确认候选", candN === 1, badgeTexts.join(","));
await shot(main, "scanned");

// ---------- 4. AI 分析（真实 API） ----------
let aiSettled = false;
let aiOk = 0;
for (let i = 0; i < 90; i++) {
  const b = await main.locator(".card .card-top .badge:nth-child(3)").allTextContents();
  const done = b.filter((t) => t.includes("已分析") || t.includes("分析失败")).length;
  const running = b.filter((t) => t.includes("分析中")).length;
  aiOk = b.filter((t) => t.includes("已分析")).length;
  if (done >= 5 && running === 0) {
    aiSettled = true;
    break;
  }
  await sleep(2000);
}
ok("AI 分析全部落定", aiSettled);
ok("AI 分析成功率 5/5", aiOk === 5, `已分析 ${aiOk}/5`);
const desc1 = await main.locator(".card").first().locator(".desc").textContent();
ok("AI 描述已回填", desc1.trim() !== "" && !desc1.includes("暂无 AI 说明"), desc1.slice(0, 40));
await shot(main, "ai-analyzed");

// ---------- 5. 详情面板 ----------
await main.locator(".card", { hasText: "resume-spider" }).first().click();
await main.waitForSelector("aside.detail");
ok("详情：标题", (await main.locator(".detail-head h2").textContent()) === "resume-spider");
const dDesc = await main.locator(".detail-desc").textContent();
ok("详情：AI 说明非空", dDesc.trim() !== "" && !dDesc.includes("暂无"));
const depSummary = await main.locator("summary").allTextContents();
ok("详情：依赖/提交摘要", depSummary.some((s) => s.includes("依赖")) && depSummary.some((s) => s.includes("最近提交")));
await shot(main, "detail");

// ---------- 6. 手动标签与备注（覆盖 AI） ----------
const tagsInput = main.locator(".field input").first();
await tagsInput.fill("爬虫, 招聘, e2e测试");
await main.getByRole("button", { name: "保存", exact: true }).click();
ok("标签保存 toast", (await waitToast(main, "标签已保存")) !== null);
await sleep(1000);
ok("标签已持久化回显", (await tagsInput.inputValue()).includes("e2e测试"), await tagsInput.inputValue());
await main.locator(".field textarea").fill("这是E2E写的备注，验证备注覆盖");
await main.getByRole("button", { name: "保存备注" }).click();
ok("备注保存 toast", (await waitToast(main, "备注已保存")) !== null);

// ---------- 7. 四种打开动作 ----------
await main.getByRole("button", { name: "复制路径" }).click();
ok("复制路径 toast", (await waitToast(main, "路径已复制")) !== null);
await main.getByRole("button", { name: "文件夹" }).click();
await sleep(1200);
ok("文件夹打开（无错误 toast）", (await waitToast(main, "失败", 500)) === null);
await main.getByRole("button", { name: "终端" }).click();
await sleep(1200);
ok("终端打开（无错误 toast）", (await waitToast(main, "失败", 500)) === null);
await main.getByRole("button", { name: "编辑器" }).click();
const edToast = await waitToast(main, "编辑器", 5000);
ok("编辑器：命令缺失有提示", edToast !== null && edToast.includes("不可用"), edToast ?? "无 toast");
await shot(main, "actions");

// ---------- 8. 搜索 ----------
await main.locator("input.search").fill("爬虫");
await sleep(700);
let names = await main.locator(".card .name").allTextContents();
ok("搜索'爬虫'命中 resume-spider", names[0] === "resume-spider", names.join(","));
await main.locator("input.search").fill("excel");
await sleep(700);
names = await main.locator(".card .name").allTextContents();
ok("搜索'excel'命中 excel-report", names.includes("excel-report") && names.length === 1, names.join(","));
await main.locator("input.search").fill("绝对不存在的词xyz");
await sleep(700);
const emptyText = (await main.locator(".empty").count())
  ? await main.locator(".empty").textContent()
  : "";
ok("无结果：提示区分于空库", emptyText.includes("没有匹配"), emptyText.slice(0, 30));
await shot(main, "search");

// ---------- 9. 候选确认 ----------
await main.locator("input.search").fill("");
await sleep(600);
await main.locator(".card", { hasText: "legacy-c-tools" }).first().click();
const confirmBtn = main.getByRole("button", { name: "确认这是项目" });
ok("候选：确认按钮存在", (await confirmBtn.count()) === 1);
await confirmBtn.click();
let candDone = false;
for (let i = 0; i < 45; i++) {
  const cards = await main.locator(".card", { hasText: "legacy-c-tools" }).count();
  if (cards > 0) {
    const b = await main
      .locator(".card", { hasText: "legacy-c-tools" })
      .locator(".card-top .badge:nth-child(3)")
      .textContent();
    if (b.includes("已分析") || b.includes("分析失败")) {
      candDone = b.includes("已分析");
      break;
    }
  }
  await sleep(2000);
}
ok("候选确认后 AI 分析完成", candDone);

// ---------- 10. 快搜窗口 ----------
// 注意：WebView2 中 document.visibilityState 对隐藏窗口可能保持 "visible"（不可靠），
// 窗口真实可见性用 Win32 IsWindowVisible（enum-windows.ps1 按进程枚举）判定。
const { execSync } = await import("node:child_process");
// 返回有标题且可见的窗口数（主窗口恒计 1；快搜可见时为 2）。纯 ASCII 输出避免编码问题。
function visibleTitledCount() {
  const out = execSync(
    `powershell -NoProfile -ExecutionPolicy Bypass -File "${process.env.USERPROFILE}\\pf-e2e-test\\enum-windows.ps1"`,
    { encoding: "utf8" },
  );
  const m = out.match(/VISIBLE_COUNT=(\d+)/);
  return m ? Number(m[1]) : null;
}
function quickWinVisible() {
  const n = visibleTitledCount();
  return n === null ? null : n >= 2;
}

if (quick) {
  await quick.waitForSelector(".quick-input", { timeout: 8000 });
  await quick.locator(".quick-input").fill("rust");
  await sleep(700);
  const qNames = await quick.locator(".quick-list .name").allTextContents();
  ok("快搜：rust 命中", qNames.some((n) => n.includes("rust-cli-tools")), qNames.join(","));
  ok("快搜：关键词高亮", (await quick.locator(".quick-list mark").count()) > 0);
  await shot(quick, "quick-window");
  await quick.locator(".quick-input").press("Escape");
  await sleep(800);
  ok("快搜：Esc 隐藏窗口（Win32）", quickWinVisible() === false);
}

// ---------- 11. 全局快捷键 Alt+Space（系统级真实按键） ----------
// 注入式按键依赖当时的前台窗口状态（UIPI 可能静默丢弃），重试 3 次以排除环境抖动。
let hotkeyOk = false;
for (let attempt = 0; attempt < 3 && !hotkeyOk; attempt++) {
  execSync(
    `powershell -NoProfile -ExecutionPolicy Bypass -File "${process.env.USERPROFILE}\\pf-e2e-test\\send-altspace.ps1" -Times 1`,
  );
  await sleep(2500);
  hotkeyOk = quickWinVisible() === true;
}
ok("全局快捷键唤起快搜（Win32）", hotkeyOk);
await shot(quick, "quick-hotkey");
await quick.locator(".quick-input").press("Escape");
await sleep(800);
ok("再次 Esc 收起（Win32）", quickWinVisible() === false);

// ---------- 汇总 ----------
console.log("\n===== 结果汇总 =====");
const failed = results.filter(([, c]) => !c);
for (const [n, c] of results) console.log(`${c ? "✓" : "✗"} ${n}`);
console.log(`\n${results.length - failed.length}/${results.length} 通过`);
process.exit(failed.length ? 1 : 0);
