# 项目快搜（Project Finder）

> 状态：V0.1 已实现（2026-10-20）。Rust 单测 81 个、前端单测 19 个全部通过。
> 实现备注：检索未用 FTS5——其默认分词器对中文子串查询不友好（trigram 需 3 字以上），
> 改为 SQLite 存储 + Rust 内存评分检索（百级项目规模下更快且对中文准确）。

## Problem Statement

如何让开发者在 1 秒内，从上百个本地项目中找到"那个做 X 的项目"，并立即以想要的方式打开它。

核心洞察：痛点不是"没分类"，而是**文件夹名和记忆之间隔着翻译**（`spider-demo-2` 三个月后自己都不认识）。Everything/Listary 只能按文件名搜；AI 分析把项目变成**按意图可搜索**的条目（"那个爬简历的项目"）。

## Recommended Direction

**Tauri 2 桌面应用**，双窗口：

1. **快搜浮层**：全局快捷键（Alt+Space）唤起的小窗，输入即搜、回车跳转——启动器式核心交互。
2. **档案柜主窗**：项目卡片库 + AI 详情页（一句话描述、技术栈、最近活跃时间）+ 手动备注（覆盖 AI 结论的信任机制）。

架构分层：

```
Rust（薄，只做必须原生的部分）
 ├─ 扫描器：遍历配置的根目录，识别项目（.git / package.json / pyproject.toml / go.mod）
 ├─ 信号提取：依赖清单、目录结构、git remote、最近 20 条 commit、README 片段
 ├─ SQLite：元数据 + FTS5 全文索引（库文件放软件运行目录，便携式；目录不可写时回退 %APPDATA%）
 ├─ 动作执行：VS Code / 资源管理器 / 复制路径 / 终端
 └─ 全局快捷键、系统托盘、开机自启

TypeScript（前端层）
 ├─ React 前端（浮层 + 档案柜两个视图）
 ├─ AI 分析器（可插拔）：默认云端 API，可切 Ollama；TS 层发起请求，
 │   结果经 invoke 写回 SQLite —— Rust 不碰网络请求，保持薄
 └─ 项目数量级为百级，向量搜索（V0.3）用 SQLite 存 embedding + 暴力余弦即可，无需向量库
```

选 Tauri 而非 Electron 的理由：常驻内存 ~30MB vs ~200MB+，浮层唤起更快，全局快捷键/托盘/自启原生支持。代价是写一点薄 Rust——扫描逻辑不复杂，可控。

## 扫描与识别规则（已定稿）

**触发**：设置中添加目录后立即自动扫描；之后每次启动时增量重扫（V0.2 前简化为每次全量重扫，已入库的不重复分析）。

**深度**：从根目录起向下最多 5 层找标记文件，每根目录可单独设置深度。跳过目录（任何深度都不进入）：`node_modules`、`.git`、`target`、`dist`、`build`、`out`、`bin`、`obj`、`vendor`、`venv`、`.venv`、`__pycache__`、`zig-cache`、`zig-out`、`.idea`、`.vs`。

**项目判定**（找到即认定为项目，**停止向下递归**——不索引项目内的嵌套子项目，monorepo 子包留给 V0.2）：

| 语言 | 强标记（单独即成立） | 弱标记（需同时有 .git） |
|---|---|---|
| Node.js | `package.json` | — |
| Python | `pyproject.toml`、`requirements.txt`、`setup.py`、`Pipfile` | — |
| Rust | `Cargo.toml` | — |
| Go | `go.mod` | — |
| Zig | `build.zig` | — |
| Java | `pom.xml`、`build.gradle(.kts)`、`settings.gradle` | — |
| C# | `*.csproj`、`*.sln`、`*.fsproj` | — |
| C/C++ | `CMakeLists.txt`、`meson.build`、`*.vcxproj` | `Makefile`、`main.c/.cpp` |
| 通用 | — | `.git`（无任何语言标记的仓库，按"未知类型"入库） |

三者皆不满足的目录不计为项目；仅弱标记命中的标记为"候选"，在档案柜里供一键确认。

**分析管线**：扫描入项目 → 信号提取（按语言取对应依赖清单：package.json 依赖 / Cargo.toml / go.mod require / requirements/pyproject / pom.xml 坐标 / .csproj 引用等；加上 README 片段、git remote、最近 20 条 commit、两层目录结构）→ AI 生成一句话说明 + 标签 → 连同原始信号写入 SQLite（FTS5）。AI 分析失败或未配置 key 时照常入库、靠 FTS 兜底，之后可批量补分析。

## Key Assumptions to Validate

- [ ] 扫描启发式覆盖率：无 git、无 manifest 的老项目会漏 → 提供"手动标记为项目"入口兜底。**待验证：扫一遍你的磁盘，看漏掉的比例。**
- [ ] AI 分析准确率：一定错一批 → 手动备注覆盖不是锦上添花，是信任机制。**待验证：全量索引一次，抽查 20 个描述的准确率。**
- [ ] 唤起摩擦：全局快捷键直接解决，这是生死线——比任何功能都重要。
- [ ] 增量重分析成本可忽略：每项目 ~2K tokens 信号，100 项目全量约 20 万 tokens（几毛钱），增量触发后几乎归零。基本确定成立。

## MVP Scope（V0.1）

- 全局快捷键快搜浮层：输入即搜、键盘导航、回车打开
- 四种打开动作：VS Code / 资源管理器 / 复制路径 / 终端
- 设置中添加扫描根目录后自动递归扫描（默认 5 层，可配置每根目录的深度）
- 项目检测 + 语言感知的信号提取（见下方"扫描与识别规则"）
- 可插拔 AI 分析器 → 一句话描述 + 标签；手动备注覆盖
- 档案柜主窗：卡片列表 + 详情页
- SQLite FTS 搜索（描述、标签、README、依赖名、commit 记录）
- 托盘常驻 + 开机自启

## Phasing

- **V0.2**：增量重分析调度（有新 commit / 依赖变化才重新分析）
- **V0.3**：语义向量搜索（本地 embeddings，自然语言查询"那个处理 Excel 的脚本"）
- **可选**：局域网访问（手机查项目）——本地服务形态白送的，Tauri 下需另起端口

## Not Doing (and Why)

- **工作现场恢复**（编辑器+终端+dev server 一键还原）——性感但过度工程，明确不做
- **项目清理建议**（重复检测/归档）——痛点真实但不进 MVP；`最后活跃时间` 字段先留着
- **分类体系/文件夹组织**——搜索取代分类，历史经验：没人维护分类
- **云同步/多机**——单机自用，不做
- **专用向量数据库**——百级项目 SQLite 暴力检索足够
- **Rust 层做重逻辑**——AI 请求、业务规则留在 TS，Rust 只做必须原生的

## Open Questions

- 默认云端模型选哪家（GLM / OpenAI / Claude；质量与价格差异不大，随手定即可）
- 项目数量级、无 git 老项目占比（决定手动标记入口的优先级）
