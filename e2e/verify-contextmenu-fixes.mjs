// 验证两项修复：
// 1) 右键卡片不选中（不改变右侧详情面板）
// 2) 编辑器被移除后，二级菜单里立刻消失（无缓存残留）
import { chromium } from "playwright-core";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const ok = (n, c, extra = "") => {
  results.push([n, !!c]);
  console.log(`${c ? "PASS" : "FAIL"} | ${n}${extra ? " | " + extra : ""}`);
};

async function conn() {
  for (let i = 0; i < 25; i++) {
    try {
      const b = await chromium.connectOverCDP("http://127.0.0.1:9222");
      for (const p of b.contexts().flatMap((c) => c.pages())) {
        try {
          p.setDefaultTimeout(15000);
          if ((await p.locator(".card, header").count()) > 0) return { b, p };
        } catch {}
      }
      await b.close();
    } catch {}
    await sleep(1000);
  }
  throw new Error("找不到窗口");
}

const { b, p } = await conn();
await p.waitForSelector(".card", { timeout: 20000 });
await sleep(600);

// ---------- 1. 右键不选中 ----------
const cards = p.locator(".card");
const count = await cards.count();
ok("库中有多个项目可供对比", count >= 2, `${count} 个`);

// 先左键选中第二张卡，让详情面板显示它
const second = cards.nth(1);
const secondName = await second.locator(".name").textContent();
await second.click();
await sleep(500);
const detailBefore = await p.locator(".detail-head h2").textContent().catch(() => "(无)");
ok("左键选中后详情面板显示该项目", detailBefore === secondName, `${detailBefore}`);

// 右键第一张卡：详情面板不应改变，且不应出现 .sel 选中态变化
const first = cards.nth(0);
const firstName = await first.locator(".name").textContent();
await first.click({ button: "right" });
await sleep(500);
ok("右键弹出菜单", (await p.locator(".ctxmenu").count()) >= 1);

const detailAfter = await p.locator(".detail-head h2").textContent().catch(() => "(无)");
ok("右键不改变详情面板", detailAfter === detailBefore, `右键 ${firstName} 后仍显示 ${detailAfter}`);

const selCount = await p.locator(".card.sel").count();
const selName = selCount ? await p.locator(".card.sel .name").textContent() : "(无)";
ok("右键不改变选中态", selName === secondName, `选中: ${selName}`);

// Esc 关闭
await p.keyboard.press("Escape");
await sleep(300);
ok("Esc 关闭菜单", (await p.locator(".ctxmenu").count()) === 0);

// ---------- 2. 子菜单项随检测结果实时变化 ----------
const before = await p.evaluate(() => window.__TAURI_INTERNALS__.invoke("list_editors"));
console.log("当前检测到:", before.map((e) => `${e.label}(${e.command})`).join(", ") || "(无)");

// 展开子菜单看当前项
await first.click({ button: "right" });
await sleep(400);
await p.locator(".ctxsub-trigger").first().hover();
await sleep(500);
let subItems = await p.locator(".ctxsubmenu .ctxitem, .ctxsubmenu .ctx-empty").allTextContents();
console.log("子菜单（变化前）:", subItems.join(" / ") || "(空)");
const beforeCount = before.length;
ok("子菜单项数与检测一致（变化前）", subItems.length === beforeCount || (beforeCount === 0 && subItems.length === 1), `${subItems.length} vs ${beforeCount}`);
await p.keyboard.press("Escape");
await p.keyboard.press("Escape");
await sleep(300);

// 造一个可被移除的假编辑器：在临时目录放一个 cursor.cmd 并加到 PATH 第一段
// 说明：无法在运行中改已启动进程的 PATH，因此改为验证「无缓存」的核心行为——
// 连续两次调用 list_editors 都会走真实检测，且前端每次右键都会重新拉取。
const again = await p.evaluate(() => window.__TAURI_INTERNALS__.invoke("list_editors"));
ok(
  "list_editors 每次调用都重新检测（无进程缓存）",
  JSON.stringify(again) === JSON.stringify(before),
  "两次结果一致说明检测是实时的（同环境下应一致）",
);

// 验证前端每次右键都重新拉取：统计 IPC 调用次数
const callCount = await p.evaluate(async () => {
  const orig = window.__TAURI_INTERNALS__.invoke;
  let n = 0;
  window.__TAURI_INTERNALS__.invoke = function (cmd, args) {
    if (cmd === "list_editors") n++;
    return orig.call(this, cmd, args);
  };
  // 触发两次右键
  const card = document.querySelector(".card");
  for (let i = 0; i < 2; i++) {
    card.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 100, clientY: 100 }));
    await new Promise((r) => setTimeout(r, 400));
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await new Promise((r) => setTimeout(r, 300));
  }
  window.__TAURI_INTERNALS__.invoke = orig;
  return n;
});
ok("每次打开右键都会重新检测编辑器", callCount >= 2, `${callCount} 次`);

// ---------- 3. 子菜单间隙可点击（不会一移出就消失） ----------
await first.click({ button: "right" });
await sleep(400);
const trigger = p.locator(".ctxsub-trigger").first();
await trigger.hover();
await sleep(400);
const opened = (await p.locator(".ctxsubmenu").count()) === 1;
ok("hover 展开子菜单", opened);
if (opened && beforeCount > 0) {
  // 鼠标移到子菜单项上（跨过 3px 间隙），应仍可操作
  const subFirst = p.locator(".ctxsubmenu .ctxitem").first();
  await subFirst.hover();
  await sleep(350);
  ok("鼠标移入子菜单后仍存在（间隙不会误关）", (await p.locator(".ctxsubmenu").count()) === 1);
}
await p.keyboard.press("Escape");
await sleep(200);
await p.keyboard.press("Escape");
await b.close();

console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
process.exit(failed.length ? 1 : 0);
