import { useEffect, useRef } from "react";

export interface MenuItem {
  label: string;
  onClick: () => void;
  danger?: boolean;
  /** 置灰但仍显示（用于说明为何不可用） */
  disabled?: boolean;
  hint?: string;
}

interface Props {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

/** 通用右键菜单：点击外部或 Esc 关闭，自动避让窗口边界。 */
export default function ContextMenu({ x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    // 捕获阶段，避免被卡片自身的点击处理吞掉
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
      {items.map((it) => (
        <button
          key={it.label}
          role="menuitem"
          className={`ctxitem${it.danger ? " danger" : ""}`}
          disabled={it.disabled}
          title={it.hint}
          onClick={() => {
            if (it.disabled) return;
            onClose();
            it.onClick();
          }}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}
