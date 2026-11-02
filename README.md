# 项目快搜 (Project Finder)

本地项目管理器：扫描电脑里的项目文件夹，用 AI 生成说明和标签，之后按"意图"搜索——
"那个爬简历的项目"、"处理 Excel 的脚本"——一秒找到，直接打开。

基于 Tauri 2（Rust 后端 + React 前端），常驻托盘，全局快捷键唤起快搜浮层。

## 功能

- **递归扫描**：设置中添加目录后自动扫描（默认向下 5 层，每目录可单独配置），
  识别 Node.js / Python / Rust / Go / Zig / Java / C# / C/C++ 项目
  （按 `package.json`、`Cargo.toml`、`go.mod`、`pyproject.toml`、`pom.xml`、`.csproj` 等标记文件判定）
- **AI 分析**：提取依赖清单、README 片段、git remote、最近 20 条提交、目录结构作为信号，
  生成一句话说明 + 标签。支持任何 OpenAI 兼容接口（GLM / OpenAI / DeepSeek…）与本地 Ollama
- **搜索**：名称 / 标签 / AI 描述 / 备注 / 依赖 / 提交记录全文检索，中文友好，
  多词 AND，按 相关度 → 最近活跃 排序
- **一键跳转**：编辑器打开（默认 `code`，可配置）/ 资源管理器 / 终端 / 复制路径
- **快搜浮层**：全局快捷键（默认 `Alt+Space`）唤起，纯键盘操作
- **数据库**：SQLite，放在软件运行目录（便携式；目录不可写时回退 `%APPDATA%\project-finder`）
- 未配置 AI 也能用：项目照常入库、关键词照常搜索，配置后可补跑分析

## 开发

```bash
npm install
npm test              # 前端单测（vitest）

npm run dev:tauri     # 开发运行（随机端口，推荐）
npm run tauri dev     # 开发运行（固定端口 1420）
```

**关于端口**：`npm run dev:tauri` 会挑一个空闲的随机端口（15000–29999），
同时通过 `PF_DEV_PORT` 喂给 vite、用 `--config` 临时覆盖 Tauri 的 `devUrl`，
两边永远一致，彻底避免和多项目/残留进程抢 1420。想指定端口：

```bash
npm run dev:tauri -- 15123
```

`npm run tauri dev` 仍走默认 1420——如果同时开发多个 Tauri 项目，用前者。

Rust 单测：

```bash
cd src-tauri
cargo test        # 82 个单测
```

E2E（真机 UI 点击测试，经 WebView2 CDP 附着真实应用）：

```bash
# 1) 造测试项目夹具 + 预置 config（脚本内已含），启动应用：
PF_DATA_DIR=<测试数据目录> WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 \
  npm run tauri dev   # 或直接跑 target/debug 的 exe（需另起 vite）
# 2) 驱动真实界面逐项点击断言：
node e2e/run.mjs    # 34 项检查，截图输出到运行目录
```

## 构建安装包

```bash
npm run tauri build   # 产出 NSIS 安装包（src-tauri/target/release/bundle/）
```

## 使用

1. 启动后先开 **设置**：添加扫描目录（如 `C:\Codes`），配置 AI（默认 GLM 接口，填 Key 即可）
2. 点 **扫描**：发现的项目自动入库并逐个 AI 分析
3. 任意界面按 `Alt+Space` 唤起快搜浮层，输入即搜：
   - `Enter` 编辑器打开 · `Alt+Enter` 文件夹 · `Ctrl+Enter` 终端 · `Ctrl+Shift+Enter` 复制路径 · `Esc` 关闭
4. 主窗口点击卡片看详情，可手动改标签、写备注（覆盖 AI 结论）
5. 关闭主窗口 = 缩到托盘；托盘右键可退出

## 备注

- `Makefile`/`main.c` 等弱标记目录（易误报）只标记为"待确认"候选，在详情页一键确认
- 只有 `.git` 没有语言标记的仓库按"未知"类型入库
- 环境变量 `PF_DATA_DIR` 可强制指定数据目录（开发/多实例用）
