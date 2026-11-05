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
    // robocopy 的退出码 0-7 都算成功，>=8 才是错误
    format!(
        r#"@echo off
chcp 65001 >nul
setlocal
set "TARGET={target}"
set "SOURCE={source}"
set "EXE={exe}"

rem 等待旧进程退出
ping -n 4 127.0.0.1 >nul

rem 镜像文件，但排除用户数据，避免覆盖配置与数据库
robocopy "%SOURCE%" "%TARGET%" /E /IS /IT /XF config.json projects.db projects.db-wal projects.db-shm /NFL /NDL /NJH /NJS /NC /NS >nul
if errorlevel 8 (
  echo 更新失败：文件复制出错
  pause
  exit /b 1
)

start "" "%TARGET%\%EXE%"

rem 自删（延迟一下确保句柄释放）
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
