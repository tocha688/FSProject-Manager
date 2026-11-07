/** Unix 秒时间戳 → 相对时间文案。now 可注入便于测试。 */
export function relativeTime(ts: number | null | undefined, now = Date.now()): string {
  if (ts == null) return "从未活跃";
  const diff = Math.floor(now / 1000 - ts);
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)} 天前`;
  if (diff < 86400 * 365) return `${Math.floor(diff / 86400 / 30)} 个月前`;
  return `${Math.floor(diff / 86400 / 365)} 年前`;
}

/** 路径过长时保留尾部（文件名最有信息量）。 */
export function truncatePath(p: string, max: number): string {
  if (max < 2) return p.slice(0, max);
  if (p.length <= max) return p;
  return "…" + p.slice(p.length - max + 1);
}

const STATUS_LABELS: Record<string, string> = {
  ok: "已分析",
  pending: "待分析",
  analyzing: "分析中",
  failed: "分析失败",
  candidate: "待确认",
  missing: "已丢失",
};

export function statusLabel(s: string): string {
  return STATUS_LABELS[s] ?? s;
}
