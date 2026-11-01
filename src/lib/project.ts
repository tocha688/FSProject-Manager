import type { Project } from "./types";

/** 待分析计数：pending 与 failed 都需要（重新）分析；candidate 需先人工确认，不计入。 */
export function countPending(list: Project[]): number {
  return list.filter((p) => p.status === "pending" || p.status === "failed").length;
}
