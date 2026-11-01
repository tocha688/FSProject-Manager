// 验证筛选功能：语言 / 类型 / 状态 三个下拉 + 结果计数 + 清除。
import { chromium } from "playwright-core";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const ok = (n, c, extra = "") => {
  results.push([n, !!c]);
  console.log(`${c ? "PASS" : "FAIL"} | ${n}${extra ? " | " + extra : ""}`);
};

const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
let main = null;
for (let i = 0; i < 30 && !main; i++) {
  for (const p of browser.contexts().flatMap((c) => c.pages())) {
    try {
      p.setDefaultTimeout(15000);
      if ((await p.locator("header").count()) > 0) { main = p; break; }
    } catch {}
  }
  if (!main) await sleep(1000);
}
if (!main) { console.log("找不到主窗口"); process.exit(1); }

await main.waitForSelector(".filterbar", { timeout: 15000 });
ok("筛选栏已渲染", (await main.locator(".filterbar").count()) === 1);

const selects = main.locator(".fl-select");
ok("三个筛选下拉都存在", (await selects.count()) === 3, `实际 ${await selects.count()}`);

const langSel = selects.nth(0);
const typeSel = selects.nth(1);
const statusSel = selects.nth(2);

// 收集选项
const langOpts = await langSel.locator("option").allTextContents();
const typeOpts = await typeSel.locator("option").allTextContents();
console.log("语言选项:", langOpts.join(" / "));
console.log("类型选项:", typeOpts.join(" / "));
ok("语言下拉有实际选项", langOpts.length > 1, `${langOpts.length} 个`);
ok("类型下拉存在「未分类」项或已有分类", typeOpts.length >= 1, `共 ${typeOpts.length} 个选项`);

// 记录当前卡片数
const totalCards = await main.locator(".card").count();
console.log("当前卡片数:", totalCards);

// ---- 语言筛选 ----
// 选第一个具体语言（跳过"全部语言"）
const firstLang = await langSel.locator("option").nth(1).getAttribute("value");
await langSel.selectOption(firstLang);
await sleep(500);
const afterLang = await main.locator(".card").count();
const allLangBadges = await main.locator(".card .badge.lang").allTextContents();
ok(
  "语言筛选生效（卡片数减少且语言一致）",
  afterLang <= totalCards && allLangBadges.every((b) => b.includes(firstLang)),
  `${firstLang}: ${afterLang} 张，徽标 ${[...new Set(allLangBadges)].join(",")}`,
);
const countText = await main.locator(".fl-count").textContent().catch(() => "");
ok("显示结果计数", countText.includes("个结果"), countText);

// ---- 组合：再叠加状态筛选 ----
const statusVals = await statusSel.locator("option").evaluateAll((os) =>
  os.map((o) => o.value).filter(Boolean),
);
await typeSel.selectOption("");
if (statusVals.length) {
  await statusSel.selectOption(statusVals[0]);
  await sleep(400);
  const comboCount = await main.locator(".card").count();
  ok("状态筛选可叠加", comboCount <= afterLang, `${comboCount} 张（原 ${afterLang}）`);
  await statusSel.selectOption("");
}

// ---- 清除筛选 ----
const clearBtn = main.locator(".fl-clear").first();
ok("存在清除筛选按钮", (await clearBtn.count()) > 0);
await clearBtn.click();
await sleep(500);
const restored = await main.locator(".card").count();
ok("清除后恢复全部", restored === totalCards, `${restored} vs ${totalCards}`);
ok("清除后按钮消失", (await main.locator(".fl-clear").count()) === 0);

// ---- 筛选 + 关键词搜索叠加 ----
const searchBox = main.locator("input.search");
const anyName = await main.locator(".card .name").first().textContent();
if (anyName) {
  const kw = anyName.slice(0, 3);
  await searchBox.fill(kw);
  await sleep(800);
  const searchCount = await main.locator(".card").count();
  ok("关键词搜索仍可用", searchCount >= 1, `「${kw}」→ ${searchCount} 张`);
  await searchBox.fill("");
  await sleep(600);
}

// ---- 类型徽标显示 ----
const typeBadges = await main.locator(".card .badge.type").allTextContents();
console.log("类型徽标:", [...new Set(typeBadges)].join(" / ") || "(无)");
ok(
  "卡片显示类型徽标（有分类时）",
  typeBadges.length > 0 || typeOpts.length <= 1,
  `${typeBadges.length} 个`,
);

console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
await browser.close();
process.exit(failed.length ? 1 : 0);
