// 验证右键「打开」二级菜单：只列出本机真实存在的编辑器/智能体。
import { chromium } from "playwright-core";

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
          if ((await p.locator(".card, .lockscreen, header").count()) > 0) return { b, p };
        } catch {}
      }
      await b.close();
    } catch {}
    await sleep(1000);
  }
  throw new Error("找不到窗口");
}

// ---------- 1. 后端检测 ----------
const { b, p } = await conn();
let detected;
try {
  detected = await p.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("list_editors"),
  );
} finally {
  await b.close();
}
console.log("检测到的编辑器:", detected.map((e) => `${e.label}(${e.command})`).join(", ") || "(无)");
ok("list_editors 返回数组", Array.isArray(detected));
ok("只返回有 command 的项", detected.every((e) => e.command && e.label && e.id));
// 本机至少应该能检测到 VS Code（前面测试中 "code" 不在 PATH，故这里只校验结构）
ok("检测结果结构与注册表一致", detected.every((e) => ["vscode","zcode","trae","cursor","windsurf","opencode","dsh","claude","aider","gemini","zed","idea","sublime","nvim"].includes(e.id)));

// ---------- 2. UI 二级菜单 ----------
const { b: b2, p: p2 } = await conn();
await p2.reload();
await p2.waitForSelector(".card", { timeout: 25000 });
await sleep(1000);

const card = p2.locator(".card").first();
await card.click({ button: "right" });
await sleep(400);
ok("右键菜单出现", (await p2.locator(".ctxmenu").count()) >= 1);

const topLabels = await p2.locator("> .ctxitem, .ctxsub-trigger").allTextContents().catch(() => []);
const allTop = await p2.locator(".ctxitem").allTextContents();
console.log("一级菜单项:", allTop.join(" / "));

ok("一级含「打开」", allTop.some((l) => l.trim().startsWith("打开") && !l.includes("文件夹")));
ok("一级含「在终端中打开」", allTop.some((l) => l.includes("终端")));
ok("一级含「打开所在文件夹」", allTop.some((l) => l.includes("所在文件夹")));
ok("一级含「复制路径」", allTop.some((l) => l.includes("复制路径")));
ok("一级含「删除项目」", allTop.some((l) => l.includes("删除")));

// 「打开」应该是可展开的子菜单触发器，而不是直接执行
const trigger = p2.locator(".ctxsub-trigger").first();
ok("「打开」是子菜单触发器", (await trigger.count()) === 1);
ok("触发器带箭头", (await trigger.locator(".ctx-arrow").count()) === 1);

// 展开子菜单
await trigger.hover();
await sleep(500);
const subVisible = (await p2.locator(".ctxsubmenu").count()) === 1;
ok("hover 展开二级菜单", subVisible);

const subItems = subVisible
  ? await p2.locator(".ctxsubmenu .ctxitem").allTextContents()
  : [];
const subEmpty = subVisible ? await p2.locator(".ctxsubmenu .ctx-empty").count() : 0;
console.log("二级菜单:", subEmpty ? "(空提示)" : subItems.join(" / "));

// 二级菜单内容必须与后端检测结果一致
if (detected.length === 0) {
  ok("无可用编辑器时显示空提示", subEmpty === 1);
} else {
  ok(
    "二级菜单项数与检测结果一致",
    subItems.length === detected.length,
    `${subItems.length} vs ${detected.length}`,
  );
  for (const e of detected) {
    ok(`二级菜单包含 ${e.label}`, subItems.some((s) => s.includes(e.label)));
  }
  // 不应出现未检测到的工具
  const notDetected = ["VS Code", "ZCode", "Trae", "Cursor", "OpenCode", "dsh", "Claude Code", "Aider", "Gemini CLI", "Zed", "IntelliJ IDEA", "Sublime Text", "Neovim"];
  for (const label of notDetected) {
    if (!detected.some((e) => e.label === label)) {
      ok(`未安装的 ${label} 不显示`, !subItems.some((s) => s.includes(label)));
    }
  }
}

// Esc 先收子菜单，再收主菜单
await p2.keyboard.press("Escape");
await sleep(250);
const subGone = (await p2.locator(".ctxsubmenu").count()) === 0;
const mainStill = (await p2.locator(".ctxmenu").count()) >= 1;
ok("Esc 先收起二级菜单", subGone);
ok("Esc 后主菜单仍在", mainStill);
await p2.keyboard.press("Escape");
await sleep(250);
ok("再按 Esc 关闭主菜单", (await p2.locator(".ctxmenu").count()) === 0);
await b2.close();

console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
process.exit(failed.length ? 1 : 0);
