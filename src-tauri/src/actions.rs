use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalKind {
    /// Windows Terminal
    WindowsTerminal,
    /// Windows 传统 cmd 窗口
    Cmd,
    /// Linux / macOS 的默认终端
    Unix,
}

pub fn detect_terminal(wt_available: bool) -> TerminalKind {
    if cfg!(windows) {
        if wt_available {
            TerminalKind::WindowsTerminal
        } else {
            TerminalKind::Cmd
        }
    } else {
        TerminalKind::Unix
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

/// 在编辑器中打开。
/// Windows 经 cmd 转发以支持 .cmd 垫片；Unix 直接执行（code/trae 等本身就是脚本）。
pub fn editor_spec(path: &str, editor_cmd: &str) -> CmdSpec {
    if cfg!(windows) {
        CmdSpec {
            program: "cmd".into(),
            args: vec!["/C".into(), editor_cmd.into(), path.into()],
        }
    } else {
        CmdSpec {
            program: editor_cmd.into(),
            args: vec![path.into()],
        }
    }
}

pub fn editor_command(path: &str, editor_cmd: &str) -> Command {
    to_command(&editor_spec(path, editor_cmd), true)
}

/// 在系统文件管理器中打开文件夹。
pub fn folder_spec(path: &str) -> CmdSpec {
    if cfg!(windows) {
        CmdSpec {
            program: "explorer".into(),
            args: vec![path.into()],
        }
    } else if cfg!(target_os = "macos") {
        CmdSpec {
            program: "open".into(),
            args: vec![path.into()],
        }
    } else {
        CmdSpec {
            program: "xdg-open".into(),
            args: vec![path.into()],
        }
    }
}

pub fn folder_command(path: &str) -> Command {
    to_command(&folder_spec(path), true)
}

/// 在终端中打开指定目录。
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
        // macOS：用 Terminal.app 打开目录
        TerminalKind::Unix if cfg!(target_os = "macos") => CmdSpec {
            program: "open".into(),
            args: vec!["-a".into(), "Terminal".into(), path.into()],
        },
        // Linux：优先尝试常见终端，逐个回落由调用方处理
        TerminalKind::Unix => CmdSpec {
            program: "x-terminal-emulator".into(),
            args: vec!["--working-directory".into(), path.into()],
        },
    }
}

pub fn terminal_command_for(kind: TerminalKind, path: &str) -> Command {
    // 终端窗口必须可见，仅在 Windows 的 cmd 回退方案里隐藏启动器本身
    to_command(&terminal_spec(kind, path), kind == TerminalKind::Cmd)
}

/// Linux 上终端模拟器名字不统一，按优先级依次尝试。
pub fn linux_terminal_candidates() -> &'static [&'static str] {
    &[
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "alacritty",
        "kitty",
        "foot",
        "xterm",
    ]
}

/// Linux 下为某个终端生成带工作目录的启动规格。
pub fn linux_terminal_spec(program: &str, path: &str) -> CmdSpec {
    let args: Vec<String> = match program {
        "gnome-terminal" | "xfce4-terminal" => {
            vec![format!("--working-directory={path}")]
        }
        "konsole" => vec!["--workdir".into(), path.into()],
        "alacritty" | "kitty" | "foot" => vec!["--working-directory".into(), path.into()],
        _ => vec!["--working-directory".into(), path.into()],
    };
    CmdSpec {
        program: program.into(),
        args,
    }
}

/// 把文本写入系统剪贴板。
/// Windows 用 clip.exe；macOS 用 pbcopy；Linux 依次尝试 wl-copy / xclip / xsel。
pub fn clipboard_spec() -> CmdSpec {
    if cfg!(windows) {
        CmdSpec {
            program: "cmd".into(),
            args: vec!["/C".into(), "clip".into()],
        }
    } else if cfg!(target_os = "macos") {
        CmdSpec {
            program: "pbcopy".into(),
            args: vec![],
        }
    } else {
        CmdSpec {
            program: "wl-copy".into(),
            args: vec![],
        }
    }
}

/// Linux 剪贴板工具的候选列表（按 Wayland → X11 的顺序）。
pub fn linux_clipboard_candidates() -> &'static [&'static str] {
    &["wl-copy", "xclip", "xsel"]
}

pub fn linux_clipboard_spec(program: &str) -> CmdSpec {
    let args: Vec<String> = match program {
        "xclip" => vec!["-selection".into(), "clipboard".into()],
        "xsel" => vec!["--clipboard".into(), "--input".into()],
        _ => vec![],
    };
    CmdSpec {
        program: program.into(),
        args,
    }
}

pub fn clipboard_command() -> Command {
    to_command(&clipboard_spec(), true)
}

/// 检测可执行名是否在 PATH 中，用于给出友好错误而不是静默失败。
/// Windows 用 `where`，Unix 用 `which`。
pub fn command_available(program: &str) -> bool {
    if program.trim().is_empty() {
        return false;
    }
    let finder = if cfg!(windows) { "where" } else { "which" };
    let mut c = Command::new(finder);
    c.arg(program);
    no_window(&mut c);
    matches!(c.output(), Ok(o) if o.status.success() && !o.stdout.is_empty())
}

/// 检测 wt 是否在 PATH 中（仅 Windows 有意义）。
pub fn wt_available() -> bool {
    cfg!(windows) && command_available("wt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_spec_is_platform_appropriate() {
        let s = editor_spec(r"?", "code");
        if cfg!(windows) {
            assert_eq!(s.program, "cmd");
            assert_eq!(s.args[0], "/C");
            assert_eq!(s.args[1], "code");
        } else {
            assert_eq!(s.program, "code");
            assert_eq!(s.args, vec!["?"]);
        }
    }

    #[test]
    fn folder_spec_uses_platform_file_manager() {
        let s = folder_spec("/tmp/x");
        if cfg!(windows) {
            assert_eq!(s.program, "explorer");
        } else if cfg!(target_os = "macos") {
            assert_eq!(s.program, "open");
        } else {
            assert_eq!(s.program, "xdg-open");
        }
        assert_eq!(s.args, vec!["/tmp/x"]);
    }

    #[test]
    fn terminal_spec_wt_uses_dash_d() {
        let s = terminal_spec(TerminalKind::WindowsTerminal, r"C:\proj");
        assert_eq!(
            s,
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
    fn terminal_spec_unix_prefers_macos_terminal_on_mac() {
        let s = terminal_spec(TerminalKind::Unix, "/tmp/p");
        if cfg!(target_os = "macos") {
            assert_eq!(s.program, "open");
            assert_eq!(s.args, vec!["-a", "Terminal", "/tmp/p"]);
        } else if !cfg!(windows) {
            assert_eq!(s.program, "x-terminal-emulator");
            assert_eq!(s.args, vec!["--working-directory", "/tmp/p"]);
        }
    }

    #[test]
    fn linux_terminal_specs_differ_by_terminal() {
        assert_eq!(
            linux_terminal_spec("konsole", "/tmp/p").args,
            vec!["--workdir", "/tmp/p"]
        );
        assert_eq!(
            linux_terminal_spec("gnome-terminal", "/tmp/p").args,
            vec!["--working-directory=/tmp/p"]
        );
        assert_eq!(
            linux_terminal_spec("alacritty", "/tmp/p").args,
            vec!["--working-directory", "/tmp/p"]
        );
    }

    #[test]
    fn linux_terminal_candidates_not_empty() {
        assert!(!linux_terminal_candidates().is_empty());
        assert!(linux_terminal_candidates().contains(&"gnome-terminal"));
    }

    #[test]
    fn clipboard_spec_is_platform_appropriate() {
        let s = clipboard_spec();
        if cfg!(windows) {
            assert_eq!(s.program, "cmd");
            assert_eq!(s.args, vec!["/C", "clip"]);
        } else if cfg!(target_os = "macos") {
            assert_eq!(s.program, "pbcopy");
        } else {
            assert_eq!(s.program, "wl-copy");
        }
    }

    #[test]
    fn linux_clipboard_specs() {
        assert_eq!(
            linux_clipboard_spec("xclip").args,
            vec!["-selection", "clipboard"]
        );
        assert_eq!(
            linux_clipboard_spec("xsel").args,
            vec!["--clipboard", "--input"]
        );
        assert!(linux_clipboard_spec("wl-copy").args.is_empty());
    }

    #[test]
    fn detect_terminal_matches_platform() {
        if cfg!(windows) {
            assert_eq!(detect_terminal(true), TerminalKind::WindowsTerminal);
            assert_eq!(detect_terminal(false), TerminalKind::Cmd);
        } else {
            assert_eq!(detect_terminal(true), TerminalKind::Unix);
            assert_eq!(detect_terminal(false), TerminalKind::Unix);
        }
    }

    #[test]
    fn command_available_finds_common_binary() {
        // 跨平台都能找到的基础命令
        #[cfg(windows)]
        assert!(command_available("cmd"));
        #[cfg(not(windows))]
        assert!(command_available("sh"));
    }

    #[test]
    fn command_available_rejects_missing_and_empty() {
        assert!(!command_available("definitely-not-a-real-binary-xyz"));
        assert!(!command_available(""));
        assert!(!command_available("   "));
    }

    #[test]
    fn wt_available_only_on_windows() {
        if cfg!(windows) {
            let _ = wt_available();
        } else {
            assert!(!wt_available(), "非 Windows 平台不应报告 wt 可用");
        }
    }
}
