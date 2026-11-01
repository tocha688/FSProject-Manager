// 验证：扫描进行中执行打开动作，UI 不阻塞。
//
// ⚠️ 安全护栏（重要）：
// 本脚本会真实启动资源管理器/终端窗口。为避免"刷屏式打开窗口"，
// 硬性限制：
//   - 最多 6 轮，每轮每类动作各 1 次（远小于人工可感知的量）
//   - 环境变量 PF_E2E_MAX_ROUNDS 可下调；超过上限直接拒绝运行
//   - 默认只跑 copy/status 这类无窗口动作，需要真实窗口时显式加 --with-windows
//
// 用法：
//   node e2e/verify-concurrent-open.mjs              # 只测 copy/status（安全）
//   node e2e/verify-concurrent-open.mjs --with-windows  # 额外测打开文件夹（最多 6 次）
import { chromium } from "playwright-core";

const WITH_WINDOWS = process.argv.includes("--with-windows");
const HARD_MAX_ROUNDS = 6;
const envMax = Number(process.env.PF_E2E_MAX_ROUNDS ?? HARD_MAX_ROUNDS);
const MAX_ROUNDS = Math.min(
  Number.isFinite(envMax) && envMax > 0 ? envMax : HARD_MAX_ROUNDS,
  HARD_MAX_ROUNDS,
);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const results = [];
const ok = (n, c, extra = "") => {
  results.push([n, !!c]);
  console.log(`${c ? "PASS" : "FAIL"} | ${n}${extra ? " | " + extra : ""}`);
};

async function call(cmd, args, timeoutMs = 25000) {
  let lastErr = "";
  for (let i = 0; i < 10; i++) {
    let browser;
    try {
      browser = await chromium.connectOverCDP("http://127.0.0.1:9222");
      for (const p of browser.contexts().flatMap((c) => c.pages())) {
        try {
          p.setDefaultTimeout(timeoutMs);
          if ((await p.locator("header").count()) === 0) continue;
          const res = await p.evaluate(
            ([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a),
            [cmd, args],
          );
          await browser.close();
          return { ok: true, value: res };
        } catch (e) {
          lastErr = String(e).slice(0, 80);
        }
      }
    } catch (e) {
      lastErr = String(e).slice(0, 80);
    }
    try { await browser?.close(); } catch {}
    await sleep(1200);
  }
  return { ok: false, error: lastErr };
}

const list = await call("list_projects");
if (!list.ok || !list.value.length) {
  console.log("无法读取项目列表:", list.error ?? "库为空");
  process.exit(1);
}
const targetId = list.value[0].id;
console.log(`项目数: ${list.value.length}，最多 ${MAX_ROUNDS} 轮，窗口动作: ${WITH_WINDOWS ? "开" : "关（--with-windows 启用）"}`);

// 触发扫描（后台执行，不等待）
await call("scan_now").catch(() => {});

const metrics = { copy: [], status: [], folder: [] };
const fails = { copy: 0, status: 0, folder: 0 };
let sawScanning = false;

for (let round = 0; round < MAX_ROUNDS; round++) {
  const actions = WITH_WINDOWS ? ["copy", "status", "folder"] : ["copy", "status"];
  for (const t of actions) {
    const t0 = Date.now();
    let r;
    if (t === "folder") r = await call("open_project", { id: targetId, target: "folder" });
    else if (t === "copy") r = await call("copy_path", { id: targetId });
    else r = await call("get_status");
    const ms = Date.now() - t0;
    if (r.ok) {
      metrics[t].push(ms);
      if (t === "status" && r.value.scanning) sawScanning = true;
    } else {
      fails[t]++;
    }
  }
  if (round === 0) await sleep(200);
}

const stat = (a) => (a.length ? Math.max(...a) : -1);
console.log("最大往返耗时:", JSON.stringify({
  copy: stat(metrics.copy),
  status: stat(metrics.status),
  ...(WITH_WINDOWS ? { folder: stat(metrics.folder) } : {}),
}));

ok("扫描期间「复制路径」无失败", fails.copy === 0, `${metrics.copy.length} 次`);
ok("扫描期间「状态查询」无失败", fails.status === 0, `${metrics.status.length} 次`);
ok("复制路径无长阻塞（<3000ms）", stat(metrics.copy) >= 0 && stat(metrics.copy) < 3000, `${stat(metrics.copy)}ms`);
ok("状态查询无长阻塞（<2000ms）", stat(metrics.status) >= 0 && stat(metrics.status) < 2000, `${stat(metrics.status)}ms`);
if (WITH_WINDOWS) {
  ok("扫描期间「打开文件夹」无失败", fails.folder === 0, `${metrics.folder.length} 次`);
  ok("打开文件夹无长阻塞（<3000ms）", stat(metrics.folder) >= 0 && stat(metrics.folder) < 3000, `${stat(metrics.folder)}ms`);
}
console.log(`扫描中被观测到: ${sawScanning ? "是" : "否（扫描可能已结束）"}`);

console.log("\n===== 汇总 =====");
const failed = results.filter(([, c]) => !c);
console.log(`${results.length - failed.length}/${results.length} 通过`);
if (failed.length) failed.forEach(([n]) => console.log("  ✗", n));
process.exit(failed.length ? 1 : 0);
