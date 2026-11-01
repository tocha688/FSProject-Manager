// 单独验证：一次扫描能否正常开始并正常结束（标志能复位）
import { chromium } from "playwright-core";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const ok = (n, c, extra = "") => {
  results.push([n, !!c]);
  console.log(`${c ? "PASS" : "FAIL"} | ${n}${extra ? " | " + extra : ""}`);
};

async function connect() {
  for (let i = 0; i < 20; i++) {
    try {
      const browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
      for (const p of browser.contexts().flatMap((c) => c.pages())) {
        try {
          p.setDefaultTimeout(20000);
          if ((await p.locator("header").count()) > 0) return { browser, page: p };
        } catch {}
      }
      await browser.close();
    } catch {}
    await sleep(1000);
  }
  throw new Error("找不到主窗口");
}

async function call(cmd, args) {
  const { browser, page } = await connect();
  try {
    return await page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
  } finally {
    await browser.close();
  }
}

// 前置检查
let st = await call("get_status", undefined);
console.log("初始状态:", JSON.stringify(st));

// 如果卡在 true，先确认这是不是上一次测试遗留
if (st.scanning) {
  console.log("标志为 true，等待其自然复位（最多 180 秒）…");
  const start = Date.now();
  while (Date.now() - start < 180000) {
    await sleep(2000);
    st = await call("get_status", undefined);
    if (!st.scanning) break;
  }
  const elapsed = Math.round((Date.now() - start) / 1000);
  ok("遗留的扫描标志能自行复位", !st.scanning, `耗时 ${elapsed}s`);
  if (st.scanning) {
    console.log("仍然卡住 → 存在真实的标志泄漏");
    process.exit(1);
  }
}

// 干净地触发一次扫描，等待完成
console.log("\n触发单次扫描…");
const t0 = Date.now();
const scanResult = await call("scan_now", undefined);
const scanMs = Date.now() - t0;
console.log(`scan_now 返回: ${JSON.stringify(scanResult)}（阻塞 ${scanMs}ms）`);
ok("scan_now 正常返回结果", scanResult && typeof scanResult.found === "number");

await sleep(500);
st = await call("get_status", undefined);
ok("扫描结束后标志已复位", st.scanning === false, JSON.stringify(st));

// 连续三次扫描，验证可以反复使用
let allOk = true;
for (let i = 0; i < 3; i++) {
  const r = await call("scan_now", undefined);
  if (!r || typeof r.found !== "number") allOk = false;
  const s = await call("get_status", undefined);
  if (s.scanning) allOk = false;
  await sleep(300);
}
ok("可连续多次扫描且标志正常复位", allOk);

console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
process.exit(failed.length ? 1 : 0);
