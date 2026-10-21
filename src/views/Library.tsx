import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import * as api from "../lib/api";
import type { AppStatus, OpenTarget, Project } from "../lib/types";
import { relativeTime, statusLabel, truncatePath } from "../lib/format";
import Settings from "./Settings";

export default function Library() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Project | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [status, setStatus] = useState<AppStatus>({ scanning: false, analyzing: false });
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
      void api.analyzePending();
      notify(`扫描完成：发现 ${r.found} 个，清理失效 ${r.pruned} 个，库中共 ${r.total} 个`);
      refresh(queryRef.current);
    } catch (e) {
      notify(String(e));
    }
  }, [notify, refresh]);

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

  if (showSettings) {
    return <Settings onBack={() => setShowSettings(false)} />;
  }

  const busy = status.scanning || status.analyzing;

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
            {status.scanning ? "扫描中…" : status.analyzing ? "AI 分析中…" : `${projects.length} 个项目`}
          </span>
          <button onClick={doScan} disabled={status.scanning}>
            扫描
          </button>
          <button onClick={addManual}>手动添加</button>
          <button onClick={() => setShowSettings(true)}>设置</button>
        </div>
      </header>

      <div className="body">
        <main className="grid">
          {projects.length === 0 && (
            <div className="empty">
              {query.trim() ? (
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
          {projects.map((p) => (
            <div
              key={p.id}
              className={`card ${selected?.id === p.id ? "sel" : ""}`}
              onClick={() => setSelected(p)}
            >
              <div className="card-top">
                <span className="name">{p.name}</span>
                <span className="badge lang">{p.language}</span>
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
