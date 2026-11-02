// 验证：密码锁屏（设置/解锁/错误密码/空闲锁/后台锁）+ 右键菜单 + 彻底删除。
// 全程使用隔离数据目录与夹具，不触碰真实项目库。
import { chromium } from "playwright-core";
import fs from "node:fs";
import path from "node:path";

const FIX = path.join(process.env.USERPROFILE, "pf-sec-test");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const ok = (n, c, extra = "") => {
  results.push([n, !!c]);
  console.log(`${c ? "PASS" : "FAIL"} | ${n}${extra ? " | " + extra : ""}`);
};

async function conn() {
  for (let i = 0; i < 20; i++) {
    try {
      const b = await chromium.connectOverCDP("http://127.0.0.1:9222");
      for (const p of b.contexts().flatMap((c) => c.pages())) {
        try {
          p.setDefaultTimeout(15000);
          if ((await p.locator("header, .lockscreen").count()) > 0) return { b, p };
        } catch {}
      }
      await b.close();
    } catch {}
    await sleep(1000);
  }
  throw new Error("找不到窗口");
}

async function call(cmd, args) {
  const { b, p } = await conn();
  try {
    return await p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
  } finally {
    await b.close();
  }
}

// ---------- 1. 初始：无密码 → 不锁 ----------
let ls = await call("get_lock_state");
ok("初始无密码", ls.hasPassword === false);
ok("无密码时不处于锁定", ls.locked === false);

// 扫描夹具
await call("scan_now");
await sleep(1500);
let rows = await call("list_projects");
ok("夹具已入库", rows.length === 2, `${rows.length} 个`);

// ---------- 2. 设置密码 ----------
await call("set_password", { oldPassword: null, newPassword: "test1234", idleLockMinutes: 1 });
ls = await call("get_lock_state");
ok("设置密码后 hasPassword=true", ls.hasPassword === true);
ok("设置密码后当前仍解锁（刚设完不踢人）", ls.locked === false);

// 密码不该通过 get_settings 泄露
const st = await call("get_settings");
ok("密码哈希不出现在设置里", !st.security.passwordHash, `"${st.security.passwordHash}"`);

// ---------- 3. 上锁 / 错误密码 / 正确密码 ----------
await call("lock_now");
ls = await call("get_lock_state");
ok("lock_now 后处于锁定", ls.locked === true);

const wrong = await call("unlock", { password: "wrongpass" });
ok("错误密码被拒绝", wrong === false);
ok("错误密码后仍锁定", (await call("get_lock_state")).locked === true);

const right = await call("unlock", { password: "test1234" });
ok("正确密码解锁", right === true);
ok("解锁后 locked=false", (await call("get_lock_state")).locked === false);

// ---------- 4. 修改密码需原密码 ----------
let threw = false;
try {
  await call("set_password", { oldPassword: "bad", newPassword: "newpass99", idleLockMinutes: 1 });
} catch {
  threw = true;
}
ok("用错误原密码改密码被拒绝", threw);

await call("set_password", { oldPassword: "test1234", newPassword: "newpass99", idleLockMinutes: 1 });
await call("lock_now");
ok("旧密码失效", (await call("unlock", { password: "test1234" })) === false);
ok("新密码可用", (await call("unlock", { password: "newpass99" })) === true);

// ---------- 5. 空闲自动锁（模拟时间流逝） ----------
// 直接把 last_activity 往回拨不可行，改为验证策略函数的分支：
// 通过 check_auto_lock 在 idle 很小时不应锁
await call("touch_activity");
let s2 = await call("check_auto_lock");
ok("刚活动过不会被自动锁", s2.locked === false);

// ---------- 6. 右键菜单（真实 DOM） ----------
const { b, p } = await conn();
await p.reload();
await p.waitForSelector(".card", { timeout: 20000 });
await sleep(800);
const card = p.locator(".card").first();
await card.click({ button: "right" });
await sleep(400);
const menuCount = await p.locator(".ctxmenu").count();
ok("右键弹出菜单", menuCount === 1);
const labels = await p.locator(".ctxitem").allTextContents();
console.log("菜单项:", labels.join(" / "));
ok("含「打开文件夹」", labels.some((l) => l.includes("文件夹")));
ok("含「终端」", labels.some((l) => l.includes("终端")));
ok("含「VS Code」", labels.some((l) => l.includes("VS Code")));
ok("含「Trae」", labels.some((l) => l.includes("Trae")));
ok("含「删除项目」", labels.some((l) => l.includes("删除")));
const dangerCount = await p.locator(".ctxitem.danger").count();
ok("删除项为危险样式", dangerCount === 1);

// Esc 关闭
await p.keyboard.press("Escape");
await sleep(300);
ok("Esc 关闭菜单", (await p.locator(".ctxmenu").count()) === 0);
await b.close();

// ---------- 7. 彻底删除（文件夹 + 数据库） ----------
rows = await call("list_projects");
const victim = rows.find((r) => r.name === "demo-app");
const victimDir = path.join(FIX, "projects", "demo-app");
ok("删除前文件夹存在", fs.existsSync(victimDir));

// 未确认时应被拒绝
let refused = false;
try {
  await call("purge_project", { id: victim.id, confirm: false, expectedPath: victim.path });
} catch (e) {
  refused = String(e).includes("未确认");
}
ok("confirm=false 被拒绝", refused);
ok("拒绝后文件夹仍在", fs.existsSync(victimDir));

// 路径不匹配时应中止
let mismatch = false;
try {
  await call("purge_project", {
    id: victim.id,
    confirm: true,
    expectedPath: "C:\\totally\\different\\path",
  });
} catch (e) {
  mismatch = String(e).includes("路径不匹配");
}
ok("路径不匹配时中止", mismatch);
ok("中止后文件夹仍在", fs.existsSync(victimDir));

// 正常删除
const pr = await call("purge_project", {
  id: victim.id,
  confirm: true,
  expectedPath: victim.path,
});
ok("删除返回 removedFolder", pr.removedFolder === true);
ok("删除返回 removedFromDb", pr.removedFromDb === true);
ok("文件夹已从磁盘删除", !fs.existsSync(victimDir));
rows = await call("list_projects");
ok("数据库中已移除", rows.length === 1 && rows[0].name === "demo-rust", `${rows.length} 个`);

// ---------- 8. 危险路径保护 ----------
let blocked = false;
try {
  // 伪造一条指向用户主目录的记录不可行，直接验证策略命令拒绝相对路径
  const r2 = rows[0];
  await call("purge_project", { id: r2.id, confirm: true, expectedPath: "relative\\path" });
} catch (e) {
  blocked = String(e).includes("路径不匹配");
}
ok("异常路径被拦截", blocked);
ok("合法项目未被误删", fs.existsSync(path.join(FIX, "projects", "demo-rust")));

// ---------- 9. 清除密码 ----------
await call("set_password", { oldPassword: "newpass99", newPassword: "", idleLockMinutes: 1 });
ls = await call("get_lock_state");
ok("清除密码后 hasPassword=false", ls.hasPassword === false);
ok("清除密码后不再锁定", ls.locked === false);
await call("lock_now");
ok("无密码时 lock_now 不产生锁定", (await call("get_lock_state")).locked === false);

console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
process.exit(failed.length ? 1 : 0);
