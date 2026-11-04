// 验证 AI 开关 bug 修复：
// 1) 点「测试连接」有进行中反馈与明确结果提示
// 2) 测试通过 → 勾选启用 → 保存 → 重新进入设置仍是启用
// 3) 未验证的配置保存时，会明确提示为什么没启用
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
          p.setDefaultTimeout(20000);
          if ((await p.locator("header").count()) > 0) return { b, p };
        } catch {}
      }
      await b.close();
    } catch {}
    await sleep(1000);
  }
  throw new Error("找不到窗口");
}

const { b, p } = await conn();
const invoke = (cmd, args) =>
  p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);

// ---------- 准备：清掉已有验证状态，模拟首次配置 ----------
// 用一个明显无效的 key 建立"未验证"状态，测试环节再填回真实 key。
const realKey = await p.evaluate(async () => {
  const s = await window.__TAURI_INTERNALS__.invoke("get_settings");
  return s.ai.apiKey;
});
console.log("原 Key 长度:", realKey.length);

await p.evaluate(async () => {
  const inv = window.__TAURI_INTERNALS__.invoke;
  const s = await inv("get_settings");
  await inv("save_settings", {
    s: {
      ...s,
      ai: { ...s.ai, apiKey: "sk-invalid-probe", enabled: false, verifiedFingerprint: "" },
    },
  });
});
await sleep(500);

await p.reload();
await p.waitForSelector("header", { timeout: 25000 });
await p.getByRole("button", { name: "设置", exact: true }).click();
await p.waitForSelector("text=AI 分析");
await sleep(600);

// ---------- 1. 未测试时开关禁用 ----------
const sw = p.locator(".switch input[type=checkbox]");
ok("初始开关被禁用", await sw.isDisabled());
const hint0 = await p.locator(".switch-hint").textContent();
ok("提示要求先测试", hint0.includes("测试连接"), hint0.trim());

// ---------- 2. 点击测试：应有「测试中…」状态 ----------
// 先把真实 Key 填回去，否则会 401
const keyField = p.locator('input[type="password"]').first();
await keyField.fill(realKey);
await sleep(300);

const testBtn = p.getByRole("button", { name: /测试连接|测试中/ });
await testBtn.click();
// 立刻检查按钮文案（可能一闪而过，用短轮询捕获）
let sawTesting = false;
for (let i = 0; i < 20; i++) {
  const t = await p.locator("button", { hasText: "测试中" }).count();
  if (t > 0) { sawTesting = true; break; }
  await sleep(100);
}
ok("点击后显示「测试中…」", sawTesting);

// 等测试完成
let toast = "";
for (let i = 0; i < 60; i++) {
  await sleep(500);
  const cnt = await p.locator(".toast").count();
  if (cnt) {
    const t = await p.locator(".toast").last().textContent();
    if (t.includes("连接成功") || t.includes("测试失败") || t.includes("请先填写")) {
      toast = t;
      break;
    }
  }
}
console.log("测试结果提示:", toast || "(无)");
ok("测试有明确结果提示", toast.length > 0);
const testPassed = toast.includes("连接成功");
ok("测试成功（含耗时）", testPassed && /\d+ms/.test(toast), toast.slice(0, 60));

if (!testPassed) {
  console.log("测试未通过，后续用例跳过");
  await b.close();
  process.exit(results.some(([, c]) => !c) ? 1 : 0);
}

// ---------- 3. 测试通过后开关应自动解锁 ----------
await sleep(600);
ok("测试通过后开关解锁", await sw.isEnabled());

// ---------- 4. 勾选启用 → 保存 ----------
await sw.check();
await sleep(200);
await p.getByRole("button", { name: "保存", exact: true }).click();
await sleep(1500);
const toastSave = (await p.locator(".toast").count())
  ? await p.locator(".toast").last().textContent()
  : "";
console.log("保存提示:", toastSave);
ok("保存提示正常（不是未启用警告）", !toastSave.includes("未能启用"), toastSave.slice(0, 60));

// ---------- 5. 关键：重新进入设置，仍应为启用 ----------
await p.getByRole("button", { name: "返回", exact: true }).click();
await sleep(800);
await p.getByRole("button", { name: "设置", exact: true }).click();
await p.waitForSelector("text=AI 分析");
await sleep(800);
const sw2 = p.locator(".switch input[type=checkbox]");
ok("重新进入后开关仍为勾选", await sw2.isChecked());
ok("重新进入后开关仍可用", await sw2.isEnabled());
const hint2 = await p.locator(".switch-hint").textContent();
ok("提示为「已启用」", hint2.includes("已启用"), hint2.trim());

// ---------- 6. 后端状态确认 ----------
const cfg = await invoke("get_settings");
ok("后端 enabled=true", cfg.ai.enabled === true);
ok("后端 aiVerified=true", cfg.aiVerified === true);
ok("指纹非空", (cfg.ai.verifiedFingerprint || "").length > 0);

// ---------- 7. 改配置后应重新要求测试 ----------
const keyInput = p.locator('input[type="password"]').first();
await keyInput.fill("sk-modified-key-for-test");
await sleep(300);
const sw3 = p.locator(".switch input[type=checkbox]");
// 改 key 后 verified 仍显示旧状态（后端判定），保存时应被关闭
await p.getByRole("button", { name: "保存", exact: true }).click();
await sleep(1500);
const cfg2 = await invoke("get_settings");
ok("改了 Key 后保存 → enabled 被关闭", cfg2.ai.enabled === false);
const toastWarn = (await p.locator(".toast").count())
  ? await p.locator(".toast").last().textContent()
  : "";
ok("并明确提示未启用原因", toastWarn.includes("未能启用") || toastWarn.includes("测试连接"), toastWarn.slice(0, 70));

// 恢复原 Key
await keyInput.fill("");
await sleep(200);
await b.close();

console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
process.exit(failed.length ? 1 : 0);
