#!/usr/bin/env node
// 随机端口开发启动器。
//
// 为什么需要它：Tauri 的 devUrl 写在 tauri.conf.json 里，端口一变就必须同步，
// 否则 Tauri 会去连旧端口一直 "Waiting for your frontend dev server"。
// 这里挑选一个空闲端口，同时通过环境变量喂给 vite（PF_DEV_PORT）
// 和 `tauri dev --config`（临时 JSON 覆盖 devUrl），两边永远一致。
//
// 用法：
//   npm run dev            # 随机端口
//   npm run dev -- 1420    # 指定端口
import { createServer } from "node:net";
import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const HOST = "127.0.0.1";
// 避开常用端口段与 Windows 动态端口区间（49152+），降低撞车概率
const RANGE = { min: 15000, max: 29999 };

/** 端口是否可绑定（可绑定 = 空闲）。 */
function isFree(port) {
  return new Promise((resolve) => {
    const srv = createServer();
    srv.once("error", () => resolve(false));
    srv.once("listening", () => srv.close(() => resolve(true)));
    srv.listen(port, HOST);
  });
}

async function findFreePort(explicit) {
  if (explicit) {
    const p = Number(explicit);
    if (!Number.isInteger(p) || p < 1 || p > 65535) {
      throw new Error(`端口不合法: ${explicit}`);
    }
    if (!(await isFree(p))) {
      throw new Error(`端口 ${p} 已被占用，请换一个或留空让脚本随机选择`);
    }
    return p;
  }
  // 随机试 60 次
  for (let i = 0; i < 60; i++) {
    const p = RANGE.min + Math.floor(Math.random() * (RANGE.max - RANGE.min + 1));
    if (await isFree(p)) return p;
  }
  throw new Error("找不到空闲端口，请手动指定：npm run dev -- <端口>");
}

const explicit = process.argv[2];
const port = await findFreePort(explicit);
const url = `http://${HOST}:${port}`;

// 临时覆盖 Tauri 配置里的 devUrl（不改动仓库中的 tauri.conf.json）
const tmpDir = mkdtempSync(join(tmpdir(), "pf-dev-"));
const override = join(tmpDir, "tauri.dev.json");
writeFileSync(override, JSON.stringify({ build: { devUrl: url } }, null, 2));

const cleanup = () => {
  try {
    rmSync(tmpDir, { recursive: true, force: true });
  } catch {
    /* 忽略 */
  }
};
process.on("exit", cleanup);
process.on("SIGINT", () => {
  cleanup();
  process.exit(130);
});

console.log(`\n  项目快搜 dev —— 前端端口: ${port}\n  打开: ${url}\n`);
if (process.env.PF_DEV_PORT && Number(process.env.PF_DEV_PORT) !== port) {
  console.log(`  (已覆盖环境变量里的 PF_DEV_PORT=${process.env.PF_DEV_PORT})\n`);
}

// Windows 下 npx 是 .cmd，必须经 shell 解析；POSIX 上直接执行避免多余 shell。
const isWin = process.platform === "win32";
const child = spawn(
  isWin ? "npx.cmd" : "npx",
  ["tauri", "dev", "--config", override],
  {
    stdio: "inherit",
    shell: isWin,
    env: { ...process.env, PF_DEV_PORT: String(port) },
  },
);

child.on("exit", (code) => {
  cleanup();
  process.exit(code ?? 0);
});
