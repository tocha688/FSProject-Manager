use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalKind {
    WindowsTerminal,
    Cmd,
}

pub fn detect_terminal(wt_available: bool) -> TerminalKind {
    if wt_available {
        TerminalKind::WindowsTerminal
    } else {
        TerminalKind::Cmd
    }
}

#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn no_window(_cmd: &mut Command) {}

/// 命令规格：程序 + 参数。构造与执行分离，规格可单测。
#[derive(Debug, Clone, PartialEq)]
pub struct CmdSpec {
    pub program: String,
    pub args: Vec<String>,
}

fn to_command(spec: &CmdSpec, hide_window: bool) -> Command {
    let mut c = Command::new(&spec.program);
    c.args(&spec.args);
    if hide_window {
        no_window(&mut c);
    }
    c
}

/// 在编辑器中打开。editor_cmd 来自设置（默认 "code"），经 cmd 转发以支持 .cmd 垫片。
pub fn editor_spec(path: &str, editor_cmd: &str) -> CmdSpec {
    CmdSpec {
        program: "cmd".into(),
        args: vec!["/C".into(), editor_cmd.into(), path.into()],
    }
}

pub fn editor_command(path: &str, editor_cmd: &str) -> Command {
    to_command(&editor_spec(path, editor_cmd), true)
}

/// 在资源管理器中打开文件夹。
pub fn folder_spec(path: &str) -> CmdSpec {
    CmdSpec {
        program: "explorer".into(),
        args: vec![path.into()],
    }
}

pub fn folder_command(path: &str) -> Command {
    to_command(&folder_spec(path), true)
}

/// 在终端打开：优先 Windows Terminal，回退到传统 cmd 窗口。
pub fn terminal_spec(kind: TerminalKind, path: &str) -> CmdSpec {
    match kind {
        TerminalKind::WindowsTerminal => CmdSpec {
            program: "wt".into(),
            args: vec!["-d".into(), path.into()],
        },
        TerminalKind::Cmd => CmdSpec {
            program: "cmd".into(),
            args: vec![
                "/C".into(),
                "start".into(),
                "ProjectFinder".into(),
                "cmd".into(),
                "/K".into(),
                format!("cd /d {path}"),
            ],
        },
    }
}

/// 注意：终端窗口本身必须是可见的；cmd 回退方案只隐藏 cmd /C 启动器本身，
/// 由 start 拉起的新 cmd 窗口仍然可见。
pub fn terminal_command_for(kind: TerminalKind, path: &str) -> Command {
    to_command(&terminal_spec(kind, path), kind == TerminalKind::Cmd)
}

/// clip.exe 从 stdin 读取内容复制到剪贴板。
pub fn clipboard_spec() -> CmdSpec {
    CmdSpec {
        program: "cmd".into(),
        args: vec!["/C".into(), "clip".into()],
    }
}

pub fn clipboard_command() -> Command {
    to_command(&clipboard_spec(), true)
}

/// 检测可执行名是否在 PATH 中（`where` 查询），用于给出友好错误而不是静默失败。
pub fn command_available(program: &str) -> bool {
    let mut c = Command::new("where");
    c.arg(program);
    no_window(&mut c);
    matches!(c.output(), Ok(o) if o.status.success() && !o.stdout.is_empty())
}

/// 检测 wt 是否在 PATH 中（结果缓存由调用方决定）。
pub fn wt_available() -> bool {
    command_available("wt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_spec_uses_cmd_and_path() {
        let s = editor_spec(r"C:\proj", "code");
        assert_eq!(
            s,
            CmdSpec {
                program: "cmd".into(),
                args: vec!["/C".into(), "code".into(), r"C:\proj".into()],
            }
        );
    }

    #[test]
    fn folder_spec_targets_explorer() {
        assert_eq!(
            folder_spec(r"C:\proj"),
            CmdSpec {
                program: "explorer".into(),
                args: vec![r"C:\proj".into()],
            }
        );
    }

    #[test]
    fn terminal_spec_wt_uses_dash_d() {
        assert_eq!(
            terminal_spec(TerminalKind::WindowsTerminal, r"C:\proj"),
            CmdSpec {
                program: "wt".into(),
                args: vec!["-d".into(), r"C:\proj".into()],
            }
        );
    }

    #[test]
    fn terminal_spec_cmd_fallback_contains_cd() {
        let s = terminal_spec(TerminalKind::Cmd, r"C:\proj");
        assert_eq!(s.program, "cmd");
        assert!(s.args.contains(&"start".to_string()));
        assert!(s.args.contains(&r"cd /d C:\proj".to_string()));
    }

    #[test]
    fn clipboard_spec_is_clip() {
        let s = clipboard_spec();
        assert_eq!(s.program, "cmd");
        assert_eq!(s.args, vec!["/C".to_string(), "clip".to_string()]);
    }

    #[test]
    fn detect_terminal_prefers_wt() {
        assert_eq!(detect_terminal(true), TerminalKind::WindowsTerminal);
        assert_eq!(detect_terminal(false), TerminalKind::Cmd);
    }

    #[test]
    fn command_available_detects_missing_binary() {
        assert!(command_available("cmd")); // 一定存在
        assert!(!command_available("definitely-not-a-real-binary-xyz"));
    }

    #[test]
    fn wt_available_does_not_panic() {
        // 环境相关，只验证可执行且不 panic
        let _ = wt_available();
    }
}
