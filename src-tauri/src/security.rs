// 锁屏策略与安全删除的纯逻辑（与 Tauri 运行时解耦，便于单测）。
use std::path::Path;

/// 是否应该因为空闲而上锁。
///
/// - `has_password` 为 false 时永不锁（没设密码就没有锁的概念）
/// - `idle_minutes` 为 0 表示关闭"空闲自动上锁"策略
/// - 其它情况：距上次活动超过阈值即锁
pub fn should_lock_on_idle(has_password: bool, idle_minutes: u32, idle_secs: i64) -> bool {
    if !has_password || idle_minutes == 0 {
        return false;
    }
    idle_secs >= (idle_minutes as i64) * 60
}

/// 是否应该因为窗口退到后台而上锁。
///
/// 需求："置于后台超过 1 分钟也要输入密码"——这里同样复用 idle_minutes 配置，
/// 让用户只需要调一个"多久算离开"的数字。
pub fn should_lock_on_background(has_password: bool, idle_minutes: u32, background_secs: i64) -> bool {
    if !has_password || idle_minutes == 0 {
        return false;
    }
    background_secs >= (idle_minutes as i64) * 60
}

/// 删除目标是否安全：必须是绝对路径、不能是盘符根目录、不能是用户主目录或其祖先，
/// 且路径深度至少 2 层。防止误删 C:\ 或 C:\Users\admin 这类灾难。
pub fn is_safe_delete_target(path: &str, home: Option<&Path>) -> bool {
    let p = Path::new(path);
    if !p.is_absolute() {
        return false;
    }
    // 至少要有两段（如 C:\foo）——盘符根 C:\ 只有一段
    let comps: Vec<_> = p.components().collect();
    if comps.len() < 2 {
        return false;
    }
    // 不能是盘符根本身
    if p.parent().is_none() {
        return false;
    }
    // 不能是用户主目录本身
    if let Some(h) = home {
        if paths_equal(p, h) {
            return false;
        }
        // 也不能是主目录的祖先
        if h.starts_with(p) {
            return false;
        }
    }
    true
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    // Windows 路径大小写不敏感
    #[cfg(windows)]
    {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn no_password_never_locks() {
        assert!(!should_lock_on_idle(false, 1, 999999));
        assert!(!should_lock_on_background(false, 1, 999999));
    }

    #[test]
    fn zero_minutes_disables_idle_lock() {
        assert!(!should_lock_on_idle(true, 0, 999999));
        assert!(!should_lock_on_background(true, 0, 999999));
    }

    #[test]
    fn locks_after_threshold() {
        // 1 分钟 = 60 秒
        assert!(!should_lock_on_idle(true, 1, 59));
        assert!(should_lock_on_idle(true, 1, 60));
        assert!(should_lock_on_idle(true, 1, 61));
        // 5 分钟
        assert!(!should_lock_on_idle(true, 5, 299));
        assert!(should_lock_on_idle(true, 5, 300));
    }

    #[test]
    fn background_uses_same_threshold() {
        assert!(!should_lock_on_background(true, 1, 59));
        assert!(should_lock_on_background(true, 1, 60));
    }

    #[test]
    fn safe_target_rejects_relative() {
        assert!(!is_safe_delete_target("foo/bar", None));
        assert!(!is_safe_delete_target("./x", None));
    }

    #[test]
    fn safe_target_rejects_drive_root() {
        assert!(!is_safe_delete_target(r"C:\", None));
        assert!(!is_safe_delete_target("C:/", None));
    }

    #[test]
    fn safe_target_rejects_home_and_ancestors() {
        let home = PathBuf::from(r"C:\Users\admin");
        assert!(!is_safe_delete_target(r"C:\Users\admin", Some(&home)));
        assert!(!is_safe_delete_target(r"C:\Users", Some(&home)));
        assert!(!is_safe_delete_target(r"C:\", Some(&home)));
    }

    #[test]
    fn safe_target_accepts_normal_project_paths() {
        let home = PathBuf::from(r"C:\Users\admin");
        assert!(is_safe_delete_target(r"C:\Codes\nodejs\my-app", Some(&home)));
        assert!(is_safe_delete_target(
            r"C:\Users\admin\pf-e2e-test\projects\demo",
            Some(&home)
        ));
        assert!(is_safe_delete_target(r"D:\work\proj", None));
    }

    #[test]
    fn safe_target_accepts_any_two_level_path() {
        assert!(is_safe_delete_target(r"C:\foo", None));
        #[cfg(not(windows))]
        assert!(is_safe_delete_target("/tmp/proj", None));
    }
}
