import { describe, expect, it } from "vitest";
import { countPending } from "./project";

const mk = (status: string) => ({
  id: 1,
  path: "x",
  name: "x",
  language: "Node.js",
  status,
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
});

describe("countPending", () => {
  it("统计 pending 与 failed 为待分析", () => {
    const list = [
      mk("pending"),
      mk("pending"),
      mk("failed"),
      mk("ok"),
      mk("candidate"),
      mk("analyzing"),
    ];
    expect(countPending(list)).toBe(3);
  });

  it("candidate 不算待分析（需用户先确认）", () => {
    expect(countPending([mk("candidate")])).toBe(0);
  });

  it("空列表为 0", () => {
    expect(countPending([])).toBe(0);
  });
});
