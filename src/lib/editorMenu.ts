import type { EditorInfo } from "./api";

export interface OpenSubItem {
  label: string;
  hint: string;
  onClick: () => void;
}

/**
 * 把后端检测到的编辑器列表转成「打开」子菜单项。
 * 纯函数：不持有缓存，每次调用都反映传入的最新检测结果，
 * 保证运行期间卸载编辑器后子菜单项立刻消失。
 */
export function buildOpenSubmenu(
  editors: EditorInfo[],
  onPick: (id: string) => void,
): OpenSubItem[] {
  return editors.map((e) => ({
    label: e.label,
    hint: e.command,
    onClick: () => onPick(e.id),
  }));
}
