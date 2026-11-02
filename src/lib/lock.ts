/** 锁屏显示的纯判定逻辑（与 UI 解耦，便于测试）。 */

export interface LockInput {
  hasPassword: boolean;
  locked: boolean;
}

/** 是否应显示锁屏界面。没设密码时不显示（等于没启用锁）。 */
export function shouldShowLock(input: LockInput): boolean {
  return input.hasPassword && input.locked;
}
