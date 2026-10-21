use crate::detect::{self, Detection};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// 任何深度都不进入的目录（依赖/构建产物/IDE 缓存）。
pub const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "bin",
    "obj",
    "vendor",
    "venv",
    "__pycache__",
    "zig-cache",
    "zig-out",
    ".gradle",
    ".idea",
    ".vs",
    ".vscode",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundProject {
    pub path: PathBuf,
    pub detection: Detection,
}

/// 读取目录条目快照（名称 + 是否目录），排序保证确定性。
/// 符号链接按非目录处理，避免循环。
pub fn dir_entries(dir: &Path) -> Option<Vec<(String, bool)>> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut v: Vec<(String, bool)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = std::fs::symlink_metadata(e.path())
            .map(|m| m.is_dir())
            .unwrap_or(false);
        v.push((name, is_dir));
    }
    v.sort_by(|a, b| a.0.cmp(&b.0));
    Some(v)
}

pub fn is_skipped(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    SKIP_DIRS
        .iter()
        .any(|s| s.eq_ignore_ascii_case(name))
}

/// 从 root 起最多向下 max_depth 层扫描项目。
/// depth 语义：root 本身是第 0 层，root 的直接子目录是第 1 层……
/// 认定为项目/候选后停止向下递归（不索引项目内嵌套项目）。
pub fn scan_root(root: &Path, max_depth: u32) -> Vec<FoundProject> {
    let mut out = Vec::new();
    walk(root, 0, max_depth, &mut out);
    out
}

fn walk(dir: &Path, depth: u32, max_depth: u32, out: &mut Vec<FoundProject>) {
    let entries = match dir_entries(dir) {
        Some(e) => e,
        None => return,
    };
    let snaps: Vec<(&str, bool)> = entries.iter().map(|(n, d)| (n.as_str(), *d)).collect();
    let detection = detect::detect(&snaps);
    if detection.is_project() || detection.is_candidate() {
        out.push(FoundProject {
            path: dir.to_path_buf(),
            detection,
        });
        return;
    }
    if depth >= max_depth {
        return;
    }
    for (name, is_dir) in entries {
        if !is_dir || is_skipped(&name) {
            continue;
        }
        walk(&dir.join(&name), depth + 1, max_depth, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(p: &Path) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, "").unwrap();
    }

    fn mkdir(p: &Path) {
        fs::create_dir_all(p).unwrap();
    }

    fn paths(found: &[FoundProject]) -> Vec<String> {
        let mut v: Vec<String> = found
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn finds_projects_at_various_depths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("alpha/package.json"));
        touch(&root.join("beta/Cargo.toml"));
        touch(&root.join("sub/deep/deeper/go.mod")); // 第 3 层
        let found = scan_root(root, 5);
        assert_eq!(
            paths(&found),
            vec!["alpha", "beta", "deeper"].iter().map(|s| s.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn respects_max_depth() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // 第 5 层（root+5）：应该找到
        touch(&root.join("a1/a2/a3/a4/a5/package.json"));
        // 第 6 层：不应该找到
        touch(&root.join("b1/b2/b3/b4/b5/b6/package.json"));
        let found = scan_root(root, 5);
        let names = paths(&found);
        assert_eq!(names, vec!["a5".to_string()]);
    }

    #[test]
    fn skips_dependency_and_build_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("node_modules/some-lib/package.json"));
        touch(&root.join("vendor/lib/go.mod"));
        touch(&root.join(".venv/proj/pyproject.toml"));
        touch(&root.join("target/debug/Cargo.toml"));
        assert!(scan_root(root, 5).is_empty());
    }

    #[test]
    fn does_not_descend_into_found_project() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("outer/package.json"));
        touch(&root.join("outer/inner/go.mod")); // 嵌套项目不单独入库
        let found = scan_root(root, 5);
        assert_eq!(paths(&found), vec!["outer".to_string()]);
    }

    #[test]
    fn candidates_are_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("legacy-c/Makefile")); // 弱标记无 git → 候选
        let found = scan_root(root, 5);
        assert_eq!(found.len(), 1);
        assert!(found[0].detection.is_candidate());
        assert_eq!(found[0].detection.language.as_deref(), Some("C/C++"));
    }

    #[test]
    fn root_itself_can_be_a_project() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("pyproject.toml"));
        let found = scan_root(root, 5);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, root);
    }

    #[test]
    fn hidden_dirs_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join(".hidden/pkg/package.json"));
        assert!(scan_root(root, 5).is_empty());
    }

    #[test]
    fn missing_root_yields_nothing() {
        assert!(scan_root(Path::new("Z:/definitely/not/here"), 5).is_empty());
    }

    #[test]
    fn is_skipped_is_case_insensitive_on_windows() {
        assert!(is_skipped("Node_Modules"));
        assert!(is_skipped(".git"));
        assert!(!is_skipped("MyProject"));
    }

    #[test]
    fn dir_entries_handles_missing_dir() {
        assert!(dir_entries(Path::new("Z:/nope")).is_none());
    }

    #[test]
    fn dir_entries_reports_dirs_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        mkdir(&root.join("sub"));
        touch(&root.join("file.txt"));
        let entries = dir_entries(root).unwrap();
        assert_eq!(
            entries,
            vec![("file.txt".to_string(), false), ("sub".to_string(), true)]
        );
    }
}
