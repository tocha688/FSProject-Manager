import { describe, expect, it } from "vitest";
import { relativeTime, statusLabel, truncatePath } from "./format";

const NOW = 1_700_000_000_000; // ms
const sec = (offsetSeconds: number) => NOW / 1000 + offsetSeconds;

describe("relativeTime", () => {
  it("null 返回从未活跃", () => {
    expect(relativeTime(null, NOW)).toBe("从未活跃");
    expect(relativeTime(undefined, NOW)).toBe("从未活跃");
  });

  it("一分钟内是刚刚", () => {
    expect(relativeTime(sec(0), NOW)).toBe("刚刚");
    expect(relativeTime(sec(-59), NOW)).toBe("刚刚");
  });

  it("分钟", () => {
    expect(relativeTime(sec(-60), NOW)).toBe("1 分钟前");
    expect(relativeTime(sec(-3600 + 1), NOW)).toBe("59 分钟前");
  });

  it("小时", () => {
    expect(relativeTime(sec(-3600), NOW)).toBe("1 小时前");
    expect(relativeTime(sec(-86400 + 1), NOW)).toBe("23 小时前");
  });

  it("天", () => {
    expect(relativeTime(sec(-86400), NOW)).toBe("1 天前");
    expect(relativeTime(sec(-86400 * 30 + 1), NOW)).toBe("29 天前");
  });

  it("月", () => {
    expect(relativeTime(sec(-86400 * 30), NOW)).toBe("1 个月前");
    expect(relativeTime(sec(-86400 * 365 + 1), NOW)).toBe("12 个月前");
  });

  it("年", () => {
    expect(relativeTime(sec(-86400 * 365), NOW)).toBe("1 年前");
    expect(relativeTime(sec(-86400 * 730), NOW)).toBe("2 年前");
  });
});

describe("truncatePath", () => {
  it("短路径原样返回", () => {
    expect(truncatePath("C:\\a", 10)).toBe("C:\\a");
  });

  it("长路径保留尾部", () => {
    expect(truncatePath("C:\\very\\long\\path\\project", 10)).toBe("…h\\project");
    expect(truncatePath("C:\\very\\long\\path\\project", 10)).toHaveLength(10);
  });

  it("max < 2 时直接截断", () => {
    expect(truncatePath("abc", 1)).toBe("a");
  });
});

describe("statusLabel", () => {
  it("已知状态映射中文", () => {
    expect(statusLabel("ok")).toBe("已分析");
    expect(statusLabel("pending")).toBe("待分析");
    expect(statusLabel("candidate")).toBe("待确认");
    expect(statusLabel("failed")).toBe("分析失败");
    expect(statusLabel("analyzing")).toBe("分析中");
  });

  it("未知状态原样返回", () => {
    expect(statusLabel("whatever")).toBe("whatever");
  });
});

describe("missing 状态", () => {
  it("已丢失显示中文", () => {
    expect(statusLabel("missing")).toBe("已丢失");
  });
});
