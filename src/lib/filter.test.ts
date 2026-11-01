import { describe, expect, it } from "vitest";
import {
  applyFilter,
  buildFacets,
  EMPTY_FILTER,
  hasFilter,
  UNTYPED,
  type FilterState,
} from "./filter";
import type { Project } from "./types";

function mk(over: Partial<Project>): Project {
  return {
    id: 1,
    path: "C:/x",
    name: "x",
    language: "Rust",
    status: "ok",
    description: null,
    tags: [],
    notes: null,
    deps: [],
    commits: [],
    readme: null,
    gitRemote: null,
    structure: [],
    lastCommitAt: null,
    analyzedAt: null,
    aiSource: null,
    updatedAt: 0,
    projectType: "",
    ...over,
  };
}

const f = (over: Partial<FilterState>): FilterState => ({ ...EMPTY_FILTER, ...over });

describe("hasFilter", () => {
  it("空条件为 false", () => {
    expect(hasFilter(EMPTY_FILTER)).toBe(false);
  });
  it("任一条件为 true", () => {
    expect(hasFilter(f({ language: "Rust" }))).toBe(true);
    expect(hasFilter(f({ type: "Web 应用" }))).toBe(true);
    expect(hasFilter(f({ status: "ok" }))).toBe(true);
  });
});

describe("applyFilter", () => {
  const list = [
    mk({ id: 1, name: "a", language: "Rust", projectType: "命令行工具" }),
    mk({ id: 2, name: "b", language: "Node.js", projectType: "Web 应用" }),
    mk({ id: 3, name: "c", language: "Rust", projectType: "" }),
    mk({ id: 4, name: "d", language: "Python", projectType: "爬虫/数据采集", status: "pending" }),
  ];

  it("无条件下原样返回", () => {
    expect(applyFilter(list, EMPTY_FILTER)).toBe(list);
  });

  it("按语言筛选", () => {
    const r = applyFilter(list, f({ language: "Rust" }));
    expect(r.map((p) => p.id)).toEqual([1, 3]);
  });

  it("按类型筛选", () => {
    const r = applyFilter(list, f({ type: "Web 应用" }));
    expect(r.map((p) => p.id)).toEqual([2]);
  });

  it("未分类只匹配已分析且无类型的项目", () => {
    const r = applyFilter(list, f({ type: UNTYPED }));
    expect(r.map((p) => p.id)).toEqual([3]);
    // pending 项目即使无类型也不算"未分类"
    expect(r.some((p) => p.id === 4)).toBe(false);
  });

  it("按状态筛选", () => {
    expect(applyFilter(list, f({ status: "pending" })).map((p) => p.id)).toEqual([4]);
  });

  it("多条件为 AND", () => {
    expect(applyFilter(list, f({ language: "Rust", type: "命令行工具" })).map((p) => p.id)).toEqual([1]);
    expect(applyFilter(list, f({ language: "Rust", type: "Web 应用" }))).toEqual([]);
  });
});

describe("buildFacets", () => {
  const list = [
    mk({ id: 1, language: "Rust", projectType: "命令行工具" }),
    mk({ id: 2, language: "Rust", projectType: "Web 应用" }),
    mk({ id: 3, language: "Node.js", projectType: "" }),
    mk({ id: 4, language: "Python", projectType: "", status: "pending" }),
  ];

  it("统计语言数量并按次数排序", () => {
    const { languages } = buildFacets(list);
    expect(languages[0]).toEqual({ name: "Rust", count: 2 });
    expect(languages.map((l) => l.name)).toContain("Node.js");
  });

  it("统计类型数量", () => {
    const { types } = buildFacets(list);
    expect(types).toHaveLength(2);
    expect(types.map((t) => t.name).sort()).toEqual(["Web 应用", "命令行工具"].sort());
  });

  it("未分类只统计已分析的", () => {
    const { untyped } = buildFacets(list);
    expect(untyped).toBe(1); // id=3 是 ok 且无类型；id=4 是 pending 不计
  });

  it("空列表返回空", () => {
    const r = buildFacets([]);
    expect(r.languages).toEqual([]);
    expect(r.types).toEqual([]);
    expect(r.untyped).toBe(0);
  });
});
