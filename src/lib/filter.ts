import type { Project } from "./types";

export interface FilterState {
  language: string; // "" = 全部
  type: string; // "" = 全部；"__untyped__" = 未分类
  status: string; // "" = 全部
}

export const EMPTY_FILTER: FilterState = { language: "", type: "", status: "" };
export const UNTYPED = "__untyped__";

/** 是否有任何筛选条件生效。 */
export function hasFilter(f: FilterState): boolean {
  return f.language !== "" || f.type !== "" || f.status !== "";
}

/**
 * 应用筛选。与关键词搜索正交：先按关键词得到结果，再在此过滤。
 * type 为 UNTYPED 时匹配"已分析但没有类型"的项目。
 */
export function applyFilter(list: Project[], f: FilterState): Project[] {
  if (!hasFilter(f)) return list;
  return list.filter((p) => {
    if (f.language && p.language !== f.language) return false;
    if (f.type) {
      if (f.type === UNTYPED) {
        if (!(p.status === "ok" && !p.projectType)) return false;
      } else if (p.projectType !== f.type) {
        return false;
      }
    }
    if (f.status && p.status !== f.status) return false;
    return true;
  });
}

export interface FacetCount {
  name: string;
  count: number;
}

/** 从项目列表统计选项与数量（用于筛选器下拉）。 */
export function buildFacets(list: Project[]): {
  languages: FacetCount[];
  types: FacetCount[];
  untyped: number;
} {
  const langs = new Map<string, number>();
  const types = new Map<string, number>();
  let untyped = 0;
  for (const p of list) {
    if (p.language) langs.set(p.language, (langs.get(p.language) ?? 0) + 1);
    if (p.projectType) types.set(p.projectType, (types.get(p.projectType) ?? 0) + 1);
    else if (p.status === "ok") untyped++;
  }
  const sort = (m: Map<string, number>): FacetCount[] =>
    [...m.entries()]
      .map(([name, count]) => ({ name, count }))
      .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name, "zh"));
  return { languages: sort(langs), types: sort(types), untyped };
}
