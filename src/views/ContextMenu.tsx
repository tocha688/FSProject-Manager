import { useEffect, useRef, useState } from "react";

export interface SubMenuItem {
  label: string;
  onClick: () => void;
  /** 右侧灰色说明（如实际命令名） */
  hint?: string;
}

export interface MenuItem {
  label: string;
  onClick?: () => void;
  danger?: boolean;
  /** 置灰但仍显示（用于说明为何不可用） */
  disabled?: boolean;
  hint?: string;
  /** 有子项时渲染为二级菜单，点击展开而不是直接执行 */
  submenu?: SubMenuItem[];
  /** 子菜单为空时的提示文案 */
  submenuEmptyHint?: string;
}

interface Props {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

/** 通用右键菜单：支持二级子菜单，点击外部或 Esc 关闭，自动避让窗口边界。 */
export default function ContextMenu({ x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [openSub, setOpenSub] = useState<number | null>(null);
  const closeTimer = useRef<number | null>(null);

  // 卸载时清掉延迟收起的定时器，避免对已卸载组件 setState
  useEffect(() => {
    return () => {
      if (closeTimer.current) window.clearTimeout(closeTimer.current);
    };
  }, []);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // 有展开的子菜单时先收起子菜单
        setOpenSub((cur) => {
          if (cur !== null) return null;
          onClose();
          return null;
        });
      }
    };
    document.addEventListener("mousedown", onDown, true);
    document.addEventListener("keydown", onKey);
    window.addEventListener("blur", onClose);
    return () => {
      document.removeEventListener("mousedown", onDown, true);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  // 避免菜单超出窗口
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const maxX = window.innerWidth - r.width - 8;
    const maxY = window.innerHeight - r.height - 8;
    if (x > maxX) el.style.left = `${Math.max(8, maxX)}px`;
    if (y > maxY) el.style.top = `${Math.max(8, maxY)}px`;
  }, [x, y]);

  return (
    <div ref={ref} className="ctxmenu" style={{ left: x, top: y }} role="menu">
      {items.map((it, i) => {
        const hasSub = Array.isArray(it.submenu);
        if (hasSub) {
          const open = openSub === i;
          const empty = (it.submenu?.length ?? 0) === 0;
          return (
            <div
              key={it.label}
              className="ctxsub-wrap"
              onMouseEnter={() => {
                if (closeTimer.current) {
                  window.clearTimeout(closeTimer.current);
                  closeTimer.current = null;
                }
                setOpenSub(i);
              }}
              onMouseLeave={() => {
                // 延迟收起：允许鼠标从父项横向移到子菜单（中间有 3px 间隙）
                closeTimer.current = window.setTimeout(() => setOpenSub(null), 180);
              }}
            >
              <button
                role="menuitem"
                className={`ctxitem ctxsub-trigger${open ? " active" : ""}`}
                onClick={() => setOpenSub(open ? null : i)}
              >
                <span>{it.label}</span>
                <span className="ctx-arrow">›</span>
              </button>
              {open && (
                <div className="ctxmenu ctxsubmenu" role="menu">
                  {empty ? (
                    <div className="ctx-empty">{it.submenuEmptyHint ?? "没有可用的项"}</div>
                  ) : (
                    it.submenu!.map((s) => (
                      <button
                        key={s.label}
                        role="menuitem"
                        className="ctxitem"
                        onClick={() => {
                          onClose();
                          s.onClick();
                        }}
                      >
                        <span>{s.label}</span>
                        {s.hint && <span className="ctx-hint">{s.hint}</span>}
                      </button>
                    ))
                  )}
                </div>
              )}
            </div>
          );
        }
        return (
          <button
            key={it.label}
            role="menuitem"
            className={`ctxitem${it.danger ? " danger" : ""}`}
            disabled={it.disabled}
            title={it.hint}
            onClick={() => {
              if (it.disabled) return;
              onClose();
              it.onClick?.();
            }}
          >
            {it.label}
          </button>
        );
      })}
    </div>
  );
}
