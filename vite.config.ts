/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    // 显式绑定 IPv4 环回：避免 localhost 被解析成 IPv6 ::1 而 Tauri 连 127.0.0.1 时握手失败
    host: "127.0.0.1",
    // 端口可由启动脚本注入（scripts/dev.mjs 会挑一个空闲随机端口后传入），
    // 未注入时回退默认值，保证直接跑 `npm run dev` 也能用。
    port: Number(process.env.PF_DEV_PORT) || 1420,
    strictPort: true,
    watch: {
      // 不监视 Rust 构建产物：Windows 上 cargo 编译时会锁住 target 下的 dll，
      // vite watcher 尝试 watch 会抛 EBUSY 直接崩溃，导致 tauri dev 起不来。
      ignored: [
        "**/src-tauri/target/**",
        "**/src-tauri/gen/**",
        "**/dist/**",
      ],
    },
  },
  build: {
    target: "chrome105",
    outDir: "dist",
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
