// 项目"移动"识别。
//
// 背景：数据库以路径为唯一键。项目从 C:\a\proj 移到 D:\b\proj 后，
// 旧记录因路径不存在会被清理，新路径被当成全新项目 ——
// AI 描述、标签、用户备注全部丢失，还要重新花钱分析。
//
// 这里用「指纹」把新旧记录对上：只要关键特征一致，就认为是同一个项目换了个位置。
use serde::Serialize;

/// 一条记录用于比对的最小特征集。
#[derive(Debug, Clone, PartialEq)]
pub struct Fingerprint {
    pub name: String,
    pub language: String,
    pub git_remote: Option<String>,
    /// 依赖列表，用于名称变化时的兜底匹配
    pub deps: Vec<String>,
}

/// 两个项目是否"同一个"（判定为移动）。
///
/// 规则（从严到宽，任一成立且**不是反向证据**即算匹配）：
/// 1. git remote URL 相同 → 几乎确定是同一个仓库（最强信号）
/// 2. 名称相同 + 语言相同 + 都没有 remote → 很可能是同一个本地项目
/// 3. 语言相同 + 依赖集合高度重合（≥70%）+ 至少 3 个依赖 → 名字改了也能认出来
///
/// 明确排除：两个 remote 都存在且不同 → 不同项目（如同名 fork）。
pub fn is_same_project(old: &Fingerprint, new: &Fingerprint) -> bool {
    let names_equal = old.name.eq_ignore_ascii_case(&new.name);
    let langs_equal = old.language == new.language;

    // 两个都有 remote 且不同 → 不是同一个（可能是同名 fork）
    if let (Some(a), Some(b)) = (&old.git_remote, &new.git_remote) {
        if !a.is_empty() && !b.is_empty() && normalize_remote(a) != normalize_remote(b) {
            return false;
        }
    }

    // 规则 1：有 remote 且相同
    if let (Some(a), Some(b)) = (&old.git_remote, &new.git_remote) {
        if !a.trim().is_empty() && !b.trim().is_empty() && normalize_remote(a) == normalize_remote(b)
        {
            return true;
        }
    }

    // 规则 2：名称 + 语言一致，且都不带 remote（纯本地项目）
    let neither_has_remote = old
        .git_remote
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
        && new
            .git_remote
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true);
    if names_equal && langs_equal && neither_has_remote {
        return true;
    }

    // 规则 3：名字变了，但同语言且依赖高度重合——比如把目录改名。
    // 要求双方各至少 3 个依赖，避免只有 1-2 个依赖的巧合。
    if langs_equal && deps_similar(&old.deps, &new.deps) {
        return true;
    }

    false
}

/// 依赖集合重合度是否达到"同一个项目"的阈值。
pub fn deps_similar(a: &[String], b: &[String]) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    let sa: std::collections::HashSet<String> =
        a.iter().map(|s| s.to_lowercase()).collect();
    let sb: std::collections::HashSet<String> =
        b.iter().map(|s| s.to_lowercase()).collect();
    let inter = sa.intersection(&sb).count();
    let union = sa.union(&sb).count();
    if union == 0 {
        return false;
    }
    // Jaccard 相似度 ≥ 0.7
    (inter as f64) / (union as f64) >= 0.7
}

/// 归一化 git remote：忽略协议差异（https / ssh / git@）与末尾 .git、斜杠。
pub fn normalize_remote(url: &str) -> String {
    let mut s = url.trim().to_lowercase();
    // git@github.com:owner/repo.git → github.com/owner/repo
    if let Some(rest) = s.strip_prefix("git@") {
        s = rest.replacen(':', "/", 1);
    }
    for prefix in ["https://", "http://", "ssh://", "git://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
            // 去掉可能的 user@
            if let Some(at) = s.find('@') {
                s = s[at + 1..].to_string();
            }
            break;
        }
    }
    s = s.trim_end_matches('/').to_string();
    if s.ends_with(".git") {
        s.truncate(s.len() - 4);
    }
    s
}

/// 待迁移的候选记录（旧路径已不存在）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveCandidate {
    pub id: i64,
    pub old_path: String,
    pub new_path: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(name: &str, lang: &str, remote: Option<&str>) -> Fingerprint {
        Fingerprint {
            name: name.into(),
            language: lang.into(),
            git_remote: remote.map(|s| s.to_string()),
            deps: vec![],
        }
    }

    fn fp_deps(name: &str, lang: &str, deps: &[&str]) -> Fingerprint {
        Fingerprint {
            name: name.into(),
            language: lang.into(),
            git_remote: None,
            deps: deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn renamed_dir_with_same_deps_is_same_project() {
        let a = fp_deps("old-name", "Node.js", &["express", "lodash", "axios", "dotenv"]);
        let b = fp_deps("new-name", "Node.js", &["express", "lodash", "axios", "dotenv"]);
        assert!(is_same_project(&a, &b), "依赖完全一致应识别为移动");
    }

    #[test]
    fn renamed_dir_with_different_deps_is_not_same() {
        let a = fp_deps("a", "Node.js", &["express", "lodash", "axios"]);
        let b = fp_deps("b", "Node.js", &["pytest", "numpy", "pandas"]);
        assert!(!is_same_project(&a, &b));
    }

    #[test]
    fn too_few_deps_not_trusted() {
        // 只有 2 个依赖时不能仅凭重合认定，避免巧合
        let a = fp_deps("a", "Node.js", &["express", "lodash"]);
        let b = fp_deps("b", "Node.js", &["express", "lodash"]);
        assert!(!is_same_project(&a, &b), "依赖太少不应据判移动");
    }

    #[test]
    fn deps_similar_threshold() {
        let a = vec!["a".into(), "b".into(), "c".into(), "d".into()];
        let b = vec!["a".into(), "b".into(), "c".into(), "x".into()];
        // 3 交集 / 5 并集 = 0.6 < 0.7 → 不相似
        assert!(!deps_similar(&a, &b));
        let c = vec!["a".into(), "b".into(), "c".into(), "d".into(), "e".into()];
        // 4/5 = 0.8 ≥ 0.7 → 相似
        assert!(deps_similar(&a, &c));
    }

    #[test]
    fn same_remote_is_same_project() {
        let a = fp("app", "Rust", Some("https://github.com/u/app.git"));
        let b = fp("app-renamed", "Rust", Some("git@github.com:u/app.git"));
        assert!(is_same_project(&a, &b), "remote 相同应判定为同一项目");
    }

    #[test]
    fn different_remote_is_not_same() {
        let a = fp("app", "Rust", Some("https://github.com/u/app.git"));
        let b = fp("app", "Rust", Some("https://github.com/other/app.git"));
        assert!(!is_same_project(&a, &b), "不同 remote 应是不同项目");
    }

    #[test]
    fn same_name_and_lang_without_remote() {
        let a = fp("my-tool", "Python", None);
        let b = fp("my-tool", "Python", None);
        assert!(is_same_project(&a, &b));
    }

    #[test]
    fn same_name_different_lang_not_same() {
        let a = fp("proj", "Rust", None);
        let b = fp("proj", "Python", None);
        assert!(!is_same_project(&a, &b));
    }

    #[test]
    fn different_name_without_remote_not_same() {
        let a = fp("aaa", "Rust", None);
        let b = fp("bbb", "Rust", None);
        assert!(!is_same_project(&a, &b));
    }

    #[test]
    fn name_match_is_case_insensitive() {
        let a = fp("MyApp", "Rust", None);
        let b = fp("myapp", "Rust", None);
        assert!(is_same_project(&a, &b));
    }

    #[test]
    fn normalize_handles_protocol_variants() {
        let variants = [
            "https://github.com/u/app.git",
            "http://github.com/u/app",
            "git@github.com:u/app.git",
            "ssh://git@github.com/u/app.git",
            "https://github.com/u/app/",
        ];
        for v in variants {
            assert_eq!(
                normalize_remote(v),
                "github.com/u/app",
                "{v} 归一化结果不符"
            );
        }
    }

    #[test]
    fn normalize_is_case_insensitive() {
        assert_eq!(
            normalize_remote("https://GitHub.com/U/App.GIT"),
            normalize_remote("git@github.com:u/app.git")
        );
    }

    #[test]
    fn one_has_remote_other_does_not() {
        // 一方有 remote、一方没有：名称相同也保守地不认作移动
        let a = fp("app", "Rust", Some("https://github.com/u/app.git"));
        let b = fp("app", "Rust", None);
        assert!(!is_same_project(&a, &b));
    }

    #[test]
    fn empty_remote_strings_treated_as_none() {
        let a = fp("app", "Rust", Some(""));
        let b = fp("app", "Rust", None);
        assert!(is_same_project(&a, &b));
    }
}
