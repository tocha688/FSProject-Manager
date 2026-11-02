import { describe, expect, it } from "vitest";
import { shouldShowLock } from "./lock";

describe("shouldShowLock", () => {
  it("没设密码永不锁", () => {
    expect(shouldShowLock({ hasPassword: false, locked: true })).toBe(false);
  });

  it("没密码且未锁 → 不显示", () => {
    expect(shouldShowLock({ hasPassword: false, locked: false })).toBe(false);
  });

  it("有密码且已锁 → 显示", () => {
    expect(shouldShowLock({ hasPassword: true, locked: true })).toBe(true);
  });

  it("有密码但已解锁 → 不显示", () => {
    expect(shouldShowLock({ hasPassword: true, locked: false })).toBe(false);
  });
});
