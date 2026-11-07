import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import * as api from "../lib/api";
import type { AppStatus, OpenTarget, Project } from "../lib/types";
import { relativeTime, statusLabel, truncatePath } from "../lib/format";
import { countPending } from "../lib/project";
import LockScreen from "./LockScreen";
import ContextMenu from "./ContextMenu";
import type { MenuItem } from "./ContextMenu";
import type { EditorInfo } from "../lib/api";
import { buildOpenSubmenu } from "../lib/editorMenu";
import {
  applyFilter,
  buildFacets,
  EMPTY_FILTER,
  hasFilter,
  UNTYPED,
  type FilterState,
} from "../lib/filter";
import Settings from "./Settings";

function AnalyzeMenu(props: {
  disabled: boolean;
  pendingCount: number;
  onPick: (mode: "pending" | "all") => void;
}) {
  const { disabled, pendingCount, onPick } = props;
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDocClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, [open]);

  const pick = (mode: "pending" | "all") => {
    setOpen(false);
    onPick(mode);
  };

  return (
    <div className="menu-wrap" ref={ref}>
      <button
        onClick={() => setOpen((o) => !o)}
        disabled={disabled}
        title="AI 分析"
        className="split-btn"
      >
        AI 分析
        {pendingCount > 0 && <span className="count-dot">{pendingCount}</span>}
        <span className="caret">▾</span>
      </button>
      {open && (
        <div className="menu" role="menu">
          <button role="menuitem" onClick={() => pick("pending")}>
            <span className="mi-title">分析未分析项目</span>
            <span className="mi-sub">
              {pendingCount > 0 ? `共 ${pendingCount} 个待分析` : "没有待分析项目"}
            </span>
          </button>
          <button role="menuitem" onClick={() => pick("all")}>
            <span className="mi-title">全部重新分析</span>
            <span className="mi-sub">覆盖已有说明与标签（保留备注）</span>
          </button>
        </div>
      )}
    </div>
  );
}

export default function Library() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Project | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [status, setStatus] = useState<AppStatus>({ scanning: false, analyzing: false });
  const [filter, setFilter] = useState<FilterState>(EMPTY_FILTER);
  // 本机可用的编辑器 / AI 智能体。每次打开右键菜单时重新检测，
  // 这样运行期间安装/卸载编辑器后，子菜单能立刻反映真实情况。
  const [editorsList, setEditorsList] = useState<EditorInfo[]>([]);
  // 锁屏
  const [locked, setLocked] = useState<boolean | null>(null); // null = 未知（加载中）

  const refreshEditors = useCallback(async () => {
    try {
      setEditorsList(await api.listEditors());
    } catch {
      setEditorsList([]);
    }
  }, []);

  useEffect(() => {
    void refreshEditors();
  }, [refreshEditors]);
  // 右键菜单
  const [ctx, setCtx] = useState<{ x: number; y: number; project: Project } | null>(null);
  const [toast, setToast] = useState("");
  const queryRef = useRef(query);
  queryRef.current = query;

  const notify = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast((t) => (t === msg ? "" : t)), 3500);
  }, []);

  const refresh = useCallback(
    async (q: string) => {
      try {
        const rows = q.trim() ? await api.searchProjects(q) : await api.listProjects();
        setProjects(rows);
        setSelected((prev) => (prev ? rows.find((r) => r.id === prev.id) ?? null : null));
      } catch (e) {
        notify(String(e));
      }
    },
    [notify],
  );

  useEffect(() => {
    refresh("");
  }, [refresh]);

  // ---- 锁屏：启动查询 + 活动上报 + 自动锁轮询 ----
  useEffect(() => {
    let alive = true;
    api
      .getLockState()
      .then((s) => alive && setLocked(s.locked))
      .catch(() => alive && setLocked(false));

    // 键鼠活动 → 上报（节流 5 秒，避免频繁 IPC）
    let lastSent = 0;
    const onActivity = () => {
      const now = Date.now();
      if (now - lastSent < 5000) return;
      lastSent = now;
      void api.touchActivity().catch(() => {});
    };
    const events = ["mousedown", "keydown", "wheel", "touchstart"] as const;
    for (const e of events) window.addEventListener(e, onActivity, { passive: true });

    // 每 10 秒检查是否该自动上锁（空闲 / 后台超时）
    const timer = window.setInterval(async () => {
      try {
        const s = await api.checkAutoLock();
        if (alive) setLocked(s.locked);
      } catch {
        /* 忽略 */
      }
    }, 10000);

    return () => {
      alive = false;
      for (const e of events) window.removeEventListener(e, onActivity);
      window.clearInterval(timer);
    };
  }, []);

  useEffect(() => {
    const t = window.setTimeout(() => refresh(query), query ? 200 : 0);
    return () => window.clearTimeout(t);
  }, [query, refresh]);

  // 轮询扫描/分析状态；从忙碌转空闲时刷新列表（AI 分析陆续回填）
  useEffect(() => {
    let wasBusy = false;
    const timer = window.setInterval(async () => {
      try {
        const s = await api.getStatus();
        setStatus(s);
        const busy = s.scanning || s.analyzing;
        if (wasBusy && !busy) refresh(queryRef.current);
        wasBusy = busy;
      } catch {
        /* 后端未就绪时静默 */
      }
    }, 1500);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const act = useCallback(
    async (id: number, target: OpenTarget) => {
      try {
        await api.openProject(id, target);
      } catch (e) {
        notify(String(e));
      }
    },
    [notify],
  );

  const copy = useCallback(
    async (id: number) => {
      try {
        await api.copyPath(id);
        notify("路径已复制到剪贴板");
      } catch (e) {
        notify(String(e));
      }
    },
    [notify],
  );

  const doScan = useCallback(async () => {
    try {
      notify("扫描中…");
      const r = await api.scanNow();
      void api.analyzePending("pending");
      notify(
        `扫描完成：发现 ${r.found} 个` +
          (r.moved > 0 ? `，识别到 ${r.moved} 个项目被移动并已恢复` : "") +
          (r.markedMissing > 0 ? `，${r.markedMissing} 个已丢失（记录已保留）` : "") +
          `，库中共 ${r.total} 个`,
      );
      refresh(queryRef.current);
    } catch (e) {
      notify(String(e));
    }
  }, [notify, refresh]);

  const doAnalyze = useCallback(
    async (mode: "pending" | "all") => {
      try {
        await api.analyzePending(mode);
        notify(mode === "all" ? "已开始全部重新分析…" : "已开始分析未分析项目…");
      } catch (e) {
        notify(String(e));
      }
    },
    [notify],
  );

  const addManual = useCallback(async () => {
    const path = await openDialog({ directory: true, multiple: false });
    if (!path) return;
    try {
      await api.addManualProject(path);
      notify("已添加并入库");
      refresh(queryRef.current);
    } catch (e) {
      notify(String(e));
    }
  }, [notify, refresh]);

  // 右键菜单里的「彻底删除」：二次确认 + 显示将要删除的路径
  const purge = useCallback(
    async (p: Project) => {
      const okToDelete = window.confirm(
        `确定要彻底删除这个项目吗？\n\n` +
          `${p.name}\n${p.path}\n\n` +
          `⚠️ 会同时删除磁盘上的整个文件夹（不进回收站），此操作不可恢复！`,
      );
      if (!okToDelete) return;
      try {
        const r = await api.purgeProject(p.id, p.path);
        notify(
          r.removedFolder
            ? `已删除项目及文件夹：${p.name}`
            : `已从库中移除（文件夹本就不存在）：${p.name}`,
        );
        if (selected?.id === p.id) setSelected(null);
        refresh(queryRef.current);
      } catch (e) {
        notify(String(e));
      }
    },
    [notify, refresh, selected],
  );

  // 构建某项目的右键菜单项
  const menuItemsFor = useCallback(
    (p: Project): MenuItem[] => [
      {
        label: "打开",
        submenu: buildOpenSubmenu(editorsList, (id) => act(p.id, id as OpenTarget)),
        submenuEmptyHint: "未检测到任何编辑器（需命令在 PATH 中）",
      },
      { label: "在终端中打开", onClick: () => act(p.id, "terminal") },
      { label: "打开所在文件夹", onClick: () => act(p.id, "folder") },
      { label: "复制路径", onClick: () => copy(p.id) },
      {
        label: "删除项目（含文件夹）",
        danger: true,
        onClick: () => void purge(p),
      },
    ],
    [act, copy, purge, editorsList],
  );

  if (showSettings) {
    return <Settings onBack={() => setShowSettings(false)} />;
  }

  // 锁屏优先于一切：未解锁时不渲染任何项目内容
  if (locked === null) {
    return (
      <div className="app">
        <p className="dim" style={{ padding: 24 }}>加载中…</p>
      </div>
    );
  }
  if (locked) {
    return <LockScreen onUnlocked={() => setLocked(false)} />;
  }

  const busy = status.scanning || status.analyzing;
  const pendingCount = countPending(projects);
  // 筛选器选项从「当前搜索结果」统计，保证下拉里的数字和实际可见结果对得上
  const facets = buildFacets(projects);
  const visible = applyFilter(projects, filter);
  const filtering = hasFilter(filter);

  return (
    <div className="app">
      <header>
        <h1>项目快搜</h1>
        <input
          className="search grow"
          placeholder="搜索：名称 / 标签 / AI 描述 / 依赖 / 提交记录…（支持中文，多词用空格）"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="actions">
          {busy && <span className="spinner" title="处理中" />}
          <span className="hint">
            {status.scanning
              ? "扫描中…（其它操作仍可正常使用）"
              : status.analyzing
                ? "AI 分析中…"
                : filtering
                  ? `${visible.length} / ${projects.length} 个项目`
                  : `${projects.length} 个项目`}
          </span>
          <button onClick={doScan} disabled={status.scanning}>
            扫描
          </button>
          <AnalyzeMenu disabled={status.analyzing} onPick={doAnalyze} pendingCount={pendingCount} />
          <button onClick={addManual}>手动添加</button>
          <button onClick={() => setShowSettings(true)}>设置</button>
        </div>
      </header>

      <div className="filterbar">
        <span className="fl-label">筛选</span>
        <select
          className="fl-select"
          value={filter.language}
          onChange={(e) => setFilter({ ...filter, language: e.target.value })}
          title="按编程语言筛选"
        >
          <option value="">全部语言</option>
          {facets.languages.map((l) => (
            <option key={l.name} value={l.name}>
              {l.name}（{l.count}）
            </option>
          ))}
        </select>
        <select
          className="fl-select"
          value={filter.type}
          onChange={(e) => setFilter({ ...filter, type: e.target.value })}
          title="按项目类型筛选"
        >
          <option value="">全部类型</option>
          {facets.types.map((t) => (
            <option key={t.name} value={t.name}>
              {t.name}（{t.count}）
            </option>
          ))}
          {facets.untyped > 0 && (
            <option value={UNTYPED}>未分类（{facets.untyped}）</option>
          )}
        </select>
        <select
          className="fl-select"
          value={filter.status}
          onChange={(e) => setFilter({ ...filter, status: e.target.value })}
          title="按分析状态筛选"
        >
          <option value="">全部状态</option>
          <option value="ok">已分析</option>
          <option value="pending">待分析</option>
          <option value="failed">分析失败</option>
          <option value="candidate">待确认</option>
          <option value="missing">已丢失</option>
        </select>
        {filtering && (
          <>
            <span className="fl-count">{visible.length} 个结果</span>
            <button className="fl-clear" onClick={() => setFilter(EMPTY_FILTER)}>
              清除筛选
            </button>
          </>
        )}
      </div>

      <div className="body">
        <main className="grid">
          {visible.length === 0 && (
            <div className="empty">
              {filtering && projects.length > 0 ? (
                <>
                  没有符合筛选条件的项目。
                  <br />
                  <button className="fl-clear inline" onClick={() => setFilter(EMPTY_FILTER)}>
                    清除筛选
                  </button>
                </>
              ) : query.trim() ? (
                <>
                  没有匹配「{query}」的项目。
                  <br />
                  换个关键词试试，或到详情页给项目补充标签/备注。
                </>
              ) : (
                <>
                  还没有任何项目。
                  <br />
                  先到「设置」里添加扫描目录，然后点「扫描」；
                  <br />
                  也可以「手动添加」单个项目文件夹。
                </>
              )}
            </div>
          )}
          {visible.map((p) => (
            <div
              key={p.id}
              className={`card ${selected?.id === p.id ? "sel" : ""} ${p.status === "missing" ? "is-missing" : ""}`}
              onClick={() => setSelected(p)}
              onContextMenu={(e) => {
                e.preventDefault();
                // 只弹菜单，不选中卡片（右侧详情面板保持原样）
                setCtx({ x: e.clientX, y: e.clientY, project: p });
                void refreshEditors();
              }}
            >
              <div className="card-top">
                <span className="name">{p.name}</span>
                <span className="badge lang">{p.language}</span>
                {p.projectType && <span className="badge type">{p.projectType}</span>}
                <span className={`badge st-${p.status}`}>{statusLabel(p.status)}</span>
              </div>
              <div className="desc">
                {p.description ?? <span className="dim">暂无 AI 说明</span>}
              </div>
              {p.tags.length > 0 && (
                <div className="tags">
                  {p.tags.slice(0, 6).map((t) => (
                    <span key={t} className="tag">
                      {t}
                    </span>
                  ))}
                </div>
              )}
              <div className="foot">
                <span className="path">{truncatePath(p.path, 46)}</span>
                <span className="dim">{relativeTime(p.lastCommitAt)}</span>
              </div>
            </div>
          ))}
        </main>

        {selected && (
          <Detail p={selected} notify={notify} onAct={act} onCopy={copy} onClose={() => setSelected(null)} onSaved={() => refresh(queryRef.current)} />
        )}
      </div>

      {ctx && (
        <ContextMenu
          x={ctx.x}
          y={ctx.y}
          items={menuItemsFor(ctx.project)}
          onClose={() => setCtx(null)}
        />
      )}
      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

function Detail(props: {
  p: Project;
  notify: (m: string) => void;
  onAct: (id: number, target: OpenTarget) => void;
  onCopy: (id: number) => void;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { p, notify, onAct, onCopy, onClose, onSaved } = props;
  const [tags, setTags] = useState(p.tags.join(", "));
  const [notes, setNotes] = useState(p.notes ?? "");

  useEffect(() => {
    setTags(p.tags.join(", "));
    setNotes(p.notes ?? "");
  }, [p]);

  const saveTags = async () => {
    const list = tags
      .split(/[,，]/)
      .map((s) => s.trim())
      .filter(Boolean);
    try {
      await api.updateTags(p.id, list);
      notify("标签已保存");
      onSaved();
    } catch (e) {
      notify(String(e));
    }
  };

  const saveNotes = async () => {
    try {
      await api.updateNotes(p.id, notes);
      notify("备注已保存");
      onSaved();
    } catch (e) {
      notify(String(e));
    }
  };

  const confirm = async () => {
    try {
      await api.confirmCandidate(p.id);
      notify("已确认，进入 AI 分析队列");
      onSaved();
    } catch (e) {
      notify(String(e));
    }
  };

  const reanalyze = async () => {
    try {
      await api.analyzeProject(p.id);
      notify("已提交 AI 分析");
      onSaved();
    } catch (e) {
      notify(String(e));
    }
  };

  const remove = async () => {
    try {
      await api.deleteProject(p.id);
      notify("已从库中移除");
      onClose();
      onSaved();
    } catch (e) {
      notify(String(e));
    }
  };

  return (
    <aside className="detail">
      <div className="detail-head">
        <h2 title={p.path}>{p.name}</h2>
        <button className="icon" onClick={onClose} aria-label="关闭">
          ✕
        </button>
      </div>
      <div className="badges">
        <span className="badge lang">{p.language}</span>
        {p.projectType && <span className="badge type">{p.projectType}</span>}
        <span className={`badge st-${p.status}`}>{statusLabel(p.status)}</span>
        <span className="dim">{relativeTime(p.lastCommitAt)}</span>
      </div>
      <p className="detail-desc">{p.description ?? "暂无 AI 说明"}</p>

      <div className="btn-row">
        <button className="primary" onClick={() => onAct(p.id, "editor")}>
          编辑器
        </button>
        <button onClick={() => onAct(p.id, "folder")}>文件夹</button>
        <button onClick={() => onAct(p.id, "terminal")}>终端</button>
        <button onClick={() => onCopy(p.id)}>复制路径</button>
      </div>

      <p className="path-full">{p.path}</p>
      {p.gitRemote && <p className="dim small">remote: {p.gitRemote}</p>}

      <label className="field">
        标签（逗号分隔，会覆盖 AI 标签）
        <div className="field-row">
          <input
            value={tags}
            onChange={(e) => setTags(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && saveTags()}
            placeholder="爬虫, 数据处理"
          />
          <button onClick={saveTags}>保存</button>
        </div>
      </label>

      <label className="field">
        我的备注（比 AI 说明优先）
        <textarea
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          rows={3}
          placeholder="记录你自己的用途描述…"
        />
        <button onClick={saveNotes}>保存备注</button>
      </label>

      {p.deps.length > 0 && (
        <details>
          <summary>依赖（{p.deps.length}）</summary>
          <div className="chips">
            {p.deps.map((d) => (
              <span key={d} className="tag">
                {d}
              </span>
            ))}
          </div>
        </details>
      )}
      {p.commits.length > 0 && (
        <details>
          <summary>最近提交（{p.commits.length}）</summary>
          <ul className="commits">
            {p.commits.map((c, i) => (
              <li key={i}>{c}</li>
            ))}
          </ul>
        </details>
      )}
      {p.readme && (
        <details>
          <summary>README 片段</summary>
          <pre className="readme">{p.readme}</pre>
        </details>
      )}
      {p.structure.length > 0 && (
        <details>
          <summary>目录结构</summary>
          <pre className="readme">{p.structure.join("\n")}</pre>
        </details>
      )}

      <div className="btn-row end">
        {p.status === "candidate" && (
          <button className="primary" onClick={confirm}>
            确认这是项目
          </button>
        )}
        {(p.status === "ok" || p.status === "failed") && (
          <button onClick={reanalyze}>重新 AI 分析</button>
        )}
        <button className="danger" onClick={remove}>
          从库中移除
        </button>
      </div>
    </aside>
  );
}
