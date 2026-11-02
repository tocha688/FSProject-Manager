// AI 编辑器 / 智能体的注册表与可用性检测。
//
// 只列出"能通过命令行启动"的工具；检测方式是在 PATH 里查命令是否存在
// （复用 actions::command_available，内部走 `where`）。
use serde::Serialize;

/// 一个可启动的编辑器/智能体定义。
pub struct EditorDef {
    /// 稳定标识，前端用它作为 open_project 的 target。
    pub id: &'static str,
    pub label: &'static str,
    /// 依次尝试的命令名；第一个存在的即使用。
    pub commands: &'static [&'static str],
}

/// 注册表：顺序即右键子菜单里的展示顺序（常用/内置优先）。
pub const EDITORS: &[EditorDef] = &[
    EditorDef {
        id: "vscode",
        label: "VS Code",
        commands: &["code"],
    },
    EditorDef {
        id: "zcode",
        label: "ZCode",
        commands: &["zcode"],
    },
    EditorDef {
        id: "trae",
        label: "Trae",
        commands: &["trae"],
    },
    EditorDef {
        id: "cursor",
        label: "Cursor",
        commands: &["cursor"],
    },
    EditorDef {
        id: "windsurf",
        label: "Windsurf",
        commands: &["windsurf"],
    },
    EditorDef {
        id: "opencode",
        label: "OpenCode",
        commands: &["opencode"],
    },
    EditorDef {
        id: "dsh",
        label: "dsh",
        commands: &["dsh"],
    },
    EditorDef {
        id: "claude",
        label: "Claude Code",
        commands: &["claude"],
    },
    EditorDef {
        id: "aider",
        label: "Aider",
        commands: &["aider"],
    },
    EditorDef {
        id: "gemini",
        label: "Gemini CLI",
        commands: &["gemini"],
    },
    EditorDef {
        id: "zed",
        label: "Zed",
        commands: &["zed"],
    },
    EditorDef {
        id: "idea",
        label: "IntelliJ IDEA",
        commands: &["idea64", "idea"],
    },
    EditorDef {
        id: "sublime",
        label: "Sublime Text",
        commands: &["subl"],
    },
    EditorDef {
        id: "nvim",
        label: "Neovim",
        commands: &["nvim"],
    },
];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EditorInfo {
    pub id: String,
    pub label: String,
    /// 实际可用的命令名（检测到的那个）。
    pub command: String,
}

/// 解析：给定可执行名查找对应的编辑器定义。
pub fn find_editor(id: &str) -> Option<&'static EditorDef> {
    EDITORS.iter().find(|e| e.id == id)
}

/// 检测给定 id 对应编辑器实际可用的命令。
/// `available` 由调用方注入（便于测试时不依赖真实 PATH）。
pub fn resolve_command(
    def: &EditorDef,
    available: impl Fn(&str) -> bool,
) -> Option<String> {
    def.commands
        .iter()
        .find(|c| available(c))
        .map(|c| (*c).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_ids_are_unique() {
        let mut ids: Vec<&str> = EDITORS.iter().map(|e| e.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "编辑器 id 不能重复");
    }

    #[test]
    fn editor_labels_are_non_empty() {
        for e in EDITORS {
            assert!(!e.label.trim().is_empty(), "{} 缺少 label", e.id);
            assert!(!e.commands.is_empty(), "{} 缺少命令", e.id);
        }
    }

    #[test]
    fn find_editor_by_id() {
        assert_eq!(find_editor("vscode").map(|e| e.label), Some("VS Code"));
        assert_eq!(find_editor("trae").map(|e| e.label), Some("Trae"));
        assert!(find_editor("nonexistent-editor").is_none());
    }

    #[test]
    fn resolve_picks_first_available_command() {
        let def = EDITORS.iter().find(|e| e.id == "idea").unwrap();
        // 只有 idea 可用时，应回退到第二个命令
        let only_idea = |c: &str| c == "idea";
        assert_eq!(resolve_command(def, only_idea).as_deref(), Some("idea"));

        // 两者都存在时优先第一个
        let both = |_: &str| true;
        assert_eq!(resolve_command(def, both).as_deref(), Some("idea64"));
    }

    #[test]
    fn resolve_returns_none_when_nothing_available() {
        let def = &EDITORS[0];
        let none = |_: &str| false;
        assert!(resolve_command(def, none).is_none());
    }

    #[test]
    fn registry_contains_requested_tools() {
        for id in ["vscode", "trae", "opencode", "zcode", "dsh"] {
            assert!(find_editor(id).is_some(), "注册表应包含 {id}");
        }
    }
}
