// 验证空闲/后台自动锁真的会触发：设密码 + 把空闲阈值设为 1 分钟，
// 然后停止上报活动，等 65 秒后 check_auto_lock 应该返回 locked=true。
import { chromium } from "playwright-core";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function conn() {
  const b = await chromium.connectOverCDP("http://127.0.0.1:9222");
  for (const p of b.contexts().flatMap((c) => c.pages())) {
    try { p.setDefaultTimeout(15000); if ((await p.locator(".lockscreen, header").count()) > 0) return { b, p }; } catch {}
  }
  await b.close();
  throw new Error("no window");
}
async function call(cmd, args) {
  const { b, p } = await conn();
  try { return await p.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]); }
  finally { await b.close(); }
}

await call("set_password", { oldPassword: null, newPassword: "idle1234", idleLockMinutes: 1 });
await call("unlock", { password: "idle1234" });
let s = await call("get_lock_state");
console.log("设好密码并解锁:", JSON.stringify(s));

console.log("停止活动上报，等待 68 秒…");
await sleep(68000);

s = await call("check_auto_lock");
console.log("check_auto_lock:", JSON.stringify(s));
const triggered = s.locked === true;
console.log(triggered ? "PASS | 空闲超时自动锁定" : "FAIL | 空闲超时未锁定");

// 解锁后清理密码，恢复测试夹具
if (triggered) {
  await call("unlock", { password: "idle1234" });
  await call("set_password", { oldPassword: "idle1234", newPassword: "", idleLockMinutes: 1 });
  console.log("已清理测试密码");
}
process.exit(triggered ? 0 : 1);
