// 便携版（绿色版）自动更新。
//
// 与安装版不同：便携版没有安装器，必须自己下载新版本、解压、替换正在运行的可执行文件。
// Windows 不允许覆盖正在运行的文件，所以用「批处理延迟替换」：
//   1. 下载 zip 到临时目录并解压
//   2. 写一个 .bat：等本进程退出 → 用新文件覆盖旧文件 → 启动新版本 → 自删
//   3. 前端提示用户后调用 exit，批处理接管
//
// 只在检测到「便携模式」时启用：exe 同级存在 portable.marker 或数据目录即 exe 目录。
use serde::Serialize;
use std::path::{Path, PathBuf};

/// 便携版运行时写入的标记文件名。打包脚本会把它放进 zip。
pub const PORTABLE_MARKER: &str = "portable.marker";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableInfo {
    /// 当前是否为便携版运行
    pub is_portable: bool,
    /// 当前可执行文件路径
    pub exe_path: String,
    /// 便携版所在目录（更新时替换整个目录内的文件）
    pub install_dir: String,
    /// 当前平台键，与 latest.json 里的键对应（如 windows-x86_64）。
    /// 由后端判断，避免前端用 navigator.platform 猜错架构。
    pub platform_key: String,
    /// 当前应用版本（读 Cargo 包版本，与安装包一致）
    pub version: String,
}

/// 生成 latest.json 使用的平台键。
pub fn platform_key() -> String {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    // 编译期架构判断，比运行时探测可靠
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    format!("{os}-{arch}")
}

/// 判断当前是否以便携版运行。
/// 依据：exe 同级存在 portable.marker。
pub fn detect(exe: &Path) -> PortableInfo {
    let dir = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
    let is_portable = dir.join(PORTABLE_MARKER).exists();
    PortableInfo {
        is_portable,
        exe_path: exe.to_string_lossy().to_string(),
        install_dir: dir.to_string_lossy().to_string(),
        platform_key: platform_key(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// 校验下载的 zip 里确实有预期的主程序，避免解压后把用户目录搞坏。
pub fn find_executable_in_dir(dir: &Path, exe_name: &str) -> Option<PathBuf> {
    let direct = dir.join(exe_name);
    if direct.is_file() {
        return Some(direct);
    }
    // 兼容 zip 里多套一层目录的情况
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let nested = p.join(exe_name);
                if nested.is_file() {
                    return Some(nested);
                }
            }
        }
    }
    None
}

/// 生成用于替换自身的 Windows 批处理脚本。
///
/// 关键点：
/// - 先等本进程退出（用 ping 延时，比 timeout 在无控制台时更可靠）
/// - 用 robocopy /MIR 镜像新目录（保留用户数据文件：config.json / projects.db）
/// - 最后启动新 exe 并自删
pub fn windows_update_script(
    install_dir: &str,
    staging_dir: &str,
    exe_name: &str,
) -> String {
    // 脚本内容必须是纯 ASCII：中文 Windows 的 cmd 按 GBK 解析 .bat，
    // UTF-8 中文会变乱码并截断 `set "TARGET=..."` 等关键行。
    //
    // 关键点：
    // 1) 必须等本进程和它的 WebView2 子进程完全退出，否则 robocopy 复制 exe 会
    //    "文件被占用"失败 —— 这是之前"更新后重启有问题"的主因。
    // 2) 用 tasklist 轮询等待，最多 30 秒，而不是固定 ping 延时。
    // 3) robocopy 失败时重试一次（杀软可能临时锁文件）。
    format!(
        r#"@echo off
setlocal enabledelayedexpansion
set "TARGET={target}"
set "SOURCE={source}"
set "EXE={exe}"
set "LOG=%TEMP%\pf-update.log"

echo [1/4] waiting for processes to exit... > "%LOG%"

set /a WAIT=0
:WAITLOOP
rem use findstr (built into Windows) instead of find, to avoid Git find.exe on PATH
tasklist /FI "IMAGENAME eq {exe}" /NH 2>nul | findstr /I /C:"{exe}" >nul
if not errorlevel 1 (
  set /a WAIT+=1
  if !WAIT! GEQ 30 (
    echo [WARN] process still running after 30s, forcing continue >> "%LOG%"
    goto COPYFILES
  )
  ping -n 2 127.0.0.1 >nul
  goto WAITLOOP
)

:COPYFILES
echo [2/4] copying new files (attempt 1)... >> "%LOG%"
robocopy "%SOURCE%" "%TARGET%" /E /IS /IT /XF config.json projects.db projects.db-wal projects.db-shm /NFL /NDL /NJH /NJS /NC /NS >> "%LOG%" 2>&1
set RC=%ERRORLEVEL%
if %RC% GEQ 8 (
  echo [RETRY] robocopy failed with %RC%, retrying... >> "%LOG%"
  ping -n 4 127.0.0.1 >nul
  robocopy "%SOURCE%" "%TARGET%" /E /IS /IT /XF config.json projects.db projects.db-wal projects.db-shm /NFL /NDL /NJH /NJS /NC /NS >> "%LOG%" 2>&1
  set RC=%ERRORLEVEL%
)
if %RC% GEQ 8 (
  echo [FAILED] robocopy exit code %RC% >> "%LOG%"
  powershell -NoProfile -Command "Add-Type -A System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show('ProjectFinder update failed (copy error %RC%). Log: %LOG%')" 2>nul
  exit /b 1
)

echo [3/4] verifying new executable... >> "%LOG%"
if not exist "%TARGET%\%EXE%" (
  echo [FAILED] target exe missing >> "%LOG%"
  powershell -NoProfile -Command "Add-Type -A System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show('ProjectFinder update failed: executable missing after copy.')" 2>nul
  exit /b 1
)

echo [4/4] starting new version... >> "%LOG%"
start "" "%TARGET%\%EXE%"

echo [OK] update applied >> "%LOG%"
(goto) 2>nul & del "%~f0"
"#,
        target = install_dir,
        source = staging_dir,
        exe = exe_name
    )
}

/// 生成 macOS / Linux 的替换脚本（shell）。
pub fn unix_update_script(install_dir: &str, staging_dir: &str, exe_name: &str) -> String {
    format!(
        r#"#!/bin/sh
# 等待旧进程退出
sleep 2
TARGET='{target}'
SOURCE='{source}'
EXE='{exe}'

# 逐个复制，保留用户数据
for f in "$SOURCE"/*; do
  base=$(basename "$f")
  case "$base" in
    config.json|projects.db|projects.db-wal|projects.db-shm) continue ;;
  esac
  cp -Rf "$f" "$TARGET/$base"
done

chmod +x "$TARGET/$EXE" 2>/dev/null
"$TARGET/$EXE" &
rm -rf "$SOURCE"
"#,
        target = install_dir,
        source = staging_dir,
        exe = exe_name
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_portable_by_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("ProjectFinder.exe");
        std::fs::write(&exe, b"x").unwrap();

        let info = detect(&exe);
        assert!(!info.is_portable, "无 marker 时不应判定为便携版");

        std::fs::write(tmp.path().join(PORTABLE_MARKER), b"portable").unwrap();
        let info = detect(&exe);
        assert!(info.is_portable, "有 marker 时应判定为便携版");
        assert_eq!(info.install_dir, tmp.path().to_string_lossy());
    }

    #[test]
    fn finds_exe_at_top_level() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("App.exe"), b"x").unwrap();
        assert!(find_executable_in_dir(tmp.path(), "App.exe").is_some());
    }

    #[test]
    fn finds_exe_in_nested_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("ProjectFinder_0.1.3_windows_x64_portable");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("App.exe"), b"x").unwrap();
        let found = find_executable_in_dir(tmp.path(), "App.exe");
        assert!(found.is_some(), "应能在嵌套目录里找到主程序");
        assert!(found.unwrap().ends_with("App.exe"));
    }

    #[test]
    fn rejects_dir_without_exe() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("readme.txt"), b"x").unwrap();
        assert!(find_executable_in_dir(tmp.path(), "App.exe").is_none());
    }

    #[test]
    fn windows_script_preserves_user_data() {
        let s = windows_update_script(r"C:\Apps\PF", r"C:\Temp\staging", "ProjectFinder.exe");
        assert!(s.contains("/XF config.json projects.db"));
        assert!(s.contains("robocopy"));
        assert!(s.contains("ProjectFinder.exe"));
    }

    #[test]
    fn unix_script_preserves_user_data() {
        let s = unix_update_script("/opt/pf", "/tmp/staging", "ProjectFinder");
        assert!(s.contains("config.json|projects.db"));
        assert!(s.contains("chmod +x"));
    }
}

#[cfg(test)]
mod platform_tests {
    use super::*;

    #[test]
    fn platform_key_matches_latest_json_format() {
        let key = platform_key();
        // 必须是 <os>-<arch> 形式，且 os/arch 在预期集合内
        let parts: Vec<&str> = key.split('-').collect();
        assert_eq!(parts.len(), 2, "平台键格式应为 os-arch: {key}");
        assert!(
            ["windows", "darwin", "linux"].contains(&parts[0]),
            "未知 OS: {}",
            parts[0]
        );
        assert!(
            ["x86_64", "aarch64"].contains(&parts[1]),
            "未知架构: {}",
            parts[1]
        );
    }

    #[test]
    fn platform_key_is_compile_time_arch() {
        // 与编译目标一致，不受运行时环境影响
        let expected_arch = if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else {
            "x86_64"
        };
        assert!(platform_key().ends_with(expected_arch));
    }

    #[test]
    fn detect_includes_platform_and_version() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("App.exe");
        std::fs::write(&exe, b"x").unwrap();
        let info = detect(&exe);
        assert!(!info.platform_key.is_empty());
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.is_portable);
    }
}

#[cfg(test)]
mod script_safety_tests {
    use super::*;

    fn win_script() -> String {
        windows_update_script(r"C:\Apps\PF", r"C:\Temp\staging", "ProjectFinder.exe")
    }

    #[test]
    fn windows_script_is_pure_ascii() {
        // 回归：中文 Windows 的 cmd 按 GBK 解析 .bat，
        // 任何非 ASCII 字符都会让脚本行被截断、整条更新失败。
        let s = win_script();
        for (i, ch) in s.chars().enumerate() {
            assert!(
                ch.is_ascii(),
                "脚本第 {i} 个字符非 ASCII: {ch:?} —— 会导致 cmd 解析错乱"
            );
        }
    }

    #[test]
    fn windows_script_excludes_user_data() {
        let s = win_script();
        assert!(s.contains("/XF config.json projects.db"), "必须排除用户数据文件");
        assert!(s.contains("projects.db-wal"));
        assert!(s.contains("projects.db-shm"));
    }

    #[test]
    fn windows_script_waits_for_process() {
        let s = win_script();
        assert!(s.contains("WAITLOOP"), "必须等待进程退出，否则复制 exe 会失败");
        assert!(s.contains("tasklist"));
        assert!(s.contains("findstr"), "用 findstr 而非 find，避免 Git 的 find.exe 冲突");
        assert!(!s.contains("| find /I"), "不应使用 find 命令");
    }

    #[test]
    fn windows_script_retries_copy() {
        let s = win_script();
        assert!(s.contains("[RETRY]"), "robocopy 失败应重试一次");
        assert!(s.contains("GEQ 8"), "robocopy 退出码 >=8 才算失败");
    }

    #[test]
    fn windows_script_verifies_before_start() {
        let s = win_script();
        assert!(s.contains("if not exist"), "启动前应校验目标 exe 存在");
        assert!(s.contains(r#"start "" "%TARGET%\%EXE%""#));
    }

    #[test]
    fn windows_script_contains_paths() {
        let s = win_script();
        assert!(s.contains(r#"set "TARGET=C:\Apps\PF""#));
        assert!(s.contains(r#"set "SOURCE=C:\Temp\staging""#));
        assert!(s.contains(r#"set "EXE=ProjectFinder.exe""#));
    }

    #[test]
    fn unix_script_is_safe() {
        let s = unix_update_script("/opt/pf", "/tmp/staging", "ProjectFinder");
        assert!(s.starts_with("#!/bin/sh"));
        assert!(s.contains("config.json|projects.db"), "必须跳过用户数据");
        assert!(s.contains("chmod +x"));
    }
}
