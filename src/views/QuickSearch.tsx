import { useEffect, useState } from "react";
import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import * as api from "../lib/api";
import type { OpenTarget, Project } from "../lib/types";
import { splitHighlight } from "../lib/highlight";
import { truncatePath } from "../lib/format";

// 快搜浮层：全局快捷键唤起，纯键盘操作。
// Enter=编辑器  Alt+Enter=文件夹  Ctrl+Enter=终端  Ctrl+Shift+Enter=复制路径  Esc=关闭
export default function QuickSearch() {
  const [q, setQ] = useState("");
  const [results, setResults] = useState<Project[]>([]);
  const [sel, setSel] = useState(0);
  const [error, setError] = useState("");
  const terms = q.trim().split(/\s+/).filter(Boolean);

  useEffect(() => {
    const t = window.setTimeout(async () => {
      try {
        const rows = q.trim() ? await api.searchProjects(q) : await api.listProjects();
        setResults(rows);
        setSel(0);
        setError("");
      } catch (e) {
        setError(String(e));
      }
    }, 120);
    return () => window.clearTimeout(t);
  }, [q]);

  // 浮层失焦自动隐藏（延迟一下避免抖动）
  useEffect(() => {
    const onBlur = () => {
      window.setTimeout(() => {
        if (!document.hasFocus()) void api.hideQuick();
      }, 150);
    };
    window.addEventListener("blur", onBlur);
    return () => window.removeEventListener("blur", onBlur);
  }, []);

  const act = async (target: OpenTarget | "copy") => {
    const p = results[sel];
    if (!p) return;
    try {
      if (target === "copy") {
        await api.copyPath(p.id);
      } else {
        await api.openProject(p.id, target);
      }
      await api.hideQuick();
    } catch (e) {
      setError(String(e));
    }
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") {
      void api.hideQuick();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(s + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(s - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (e.ctrlKey && e.shiftKey) void act("copy");
      else if (e.ctrlKey) void act("terminal");
      else if (e.altKey) void act("folder");
      else void act("editor");
    }
  };

  return (
    <div className="quick">
      <input
        className="quick-input"
        autoFocus
        placeholder="搜索项目…"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={onKeyDown}
        spellCheck={false}
      />
      <ul className="quick-list">
        {results.map((p, i) => (
          <li
            key={p.id}
            className={i === sel ? "sel" : ""}
            onMouseEnter={() => setSel(i)}
            onClick={() => void act("editor")}
          >
            <div className="line1">
              <span className="name">
                {splitHighlight(p.name, terms).map((seg, j) =>
                  seg.hit ? <mark key={j}>{seg.text}</mark> : <span key={j}>{seg.text}</span>,
                )}
              </span>
              <span className="badge lang">{p.language}</span>
              {p.tags.slice(0, 3).map((t) => (
                <span key={t} className="tag">
                  {t}
                </span>
              ))}
            </div>
            <div className="line2 dim">
              {p.description ?? truncatePath(p.path, 70)}
            </div>
          </li>
        ))}
        {results.length === 0 && <li className="none dim">没有匹配的项目</li>}
      </ul>
      <footer className="quick-foot">
        <span>Enter 编辑器</span>
        <span>Alt+Enter 文件夹</span>
        <span>Ctrl+Enter 终端</span>
        <span>Ctrl+Shift+Enter 复制路径</span>
        <span>Esc 关闭</span>
      </footer>
      {error && <div className="quick-err">{error}</div>}
    </div>
  );
}
