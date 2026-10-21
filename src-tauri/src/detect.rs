use serde::Serialize;

pub const UNKNOWN_LANG: &str = "未知";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    Strong,
    Weak,
}

/// 强标记：文件单独出现即认定为项目。数组顺序 = 多语言并存时的优先序。
const STRONG_MARKERS: &[(&str, &str)] = &[
    ("package.json", "Node.js"),
    ("Cargo.toml", "Rust"),
    ("go.mod", "Go"),
    ("pyproject.toml", "Python"),
    ("requirements.txt", "Python"),
    ("setup.py", "Python"),
    ("Pipfile", "Python"),
    ("pom.xml", "Java"),
    ("build.gradle", "Java"),
    ("build.gradle.kts", "Java"),
    ("settings.gradle", "Java"),
    ("settings.gradle.kts", "Java"),
    ("build.zig", "Zig"),
    ("CMakeLists.txt", "C/C++"),
    ("meson.build", "C/C++"),
];

/// 按后缀匹配的强标记（文件名不固定）。
const STRONG_SUFFIXES: &[(&str, &str)] = &[
    (".csproj", "C#"),
    (".fsproj", "C#"),
    (".sln", "C#"),
    (".vcxproj", "C/C++"),
];

/// 弱标记：容易误报，需同时存在 .git 目录才认定为项目。
const WEAK_MARKERS: &[(&str, &str)] = &[
    ("Makefile", "C/C++"),
    ("main.c", "C/C++"),
    ("main.cpp", "C/C++"),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub language: Option<String>,
    pub strength: Option<Strength>,
    pub has_git: bool,
}

impl Detection {
    /// 项目成立条件：强标记，或有 .git（覆盖无语言标记的仓库）。
    pub fn is_project(&self) -> bool {
        self.strength == Some(Strength::Strong) || self.has_git
    }
    /// 候选：只有弱标记且无 .git，需用户在界面里确认。
    pub fn is_candidate(&self) -> bool {
        !self.is_project() && self.strength == Some(Strength::Weak)
    }
    pub fn status(&self) -> &'static str {
        if self.is_candidate() {
            "candidate"
        } else {
            "pending"
        }
    }
}

fn strong_lang(name: &str) -> Option<&'static str> {
    for (marker, lang) in STRONG_MARKERS {
        if marker.eq_ignore_ascii_case(name) {
            return Some(lang);
        }
    }
    for (suffix, lang) in STRONG_SUFFIXES {
        if name.len() > suffix.len() && name.to_lowercase().ends_with(suffix) {
            return Some(lang);
        }
    }
    None
}

fn weak_lang(name: &str) -> Option<&'static str> {
    for (marker, lang) in WEAK_MARKERS {
        if marker.eq_ignore_ascii_case(name) {
            return Some(lang);
        }
    }
    None
}

/// entries: 目录下所有条目的 (名称, 是否为目录) 快照。
pub fn detect(entries: &[(&str, bool)]) -> Detection {
    let has_git = entries.iter().any(|(n, d)| *d && *n == ".git");

    // 按语言统计强标记数量，数量多者胜（同一仓库混有多语言痕迹时更准）。
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for (name, is_dir) in entries {
        if *is_dir {
            continue;
        }
        if let Some(lang) = strong_lang(name) {
            match counts.iter_mut().find(|(l, _)| *l == lang) {
                Some((_, c)) => *c += 1,
                None => counts.push((lang, 1)),
            }
        }
    }
    let strong = counts.iter().max_by_key(|(_, c)| *c).map(|(l, _)| *l);

    let weak = entries
        .iter()
        .filter(|(_, d)| !*d)
        .find_map(|(n, _)| weak_lang(n));

    let (language, strength) = if let Some(lang) = strong {
        (Some(lang), Some(Strength::Strong))
    } else if let Some(lang) = weak {
        (Some(lang), Some(Strength::Weak))
    } else if has_git {
        (Some(UNKNOWN_LANG), None)
    } else {
        (None, None)
    };

    Detection {
        language: language.map(String::from),
        strength,
        has_git,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(entries: &[(&str, bool)]) -> Detection {
        detect(entries)
    }

    #[test]
    fn nodejs_strong_marker() {
        let x = d(&[("package.json", false), ("src", true)]);
        assert_eq!(x.language.as_deref(), Some("Node.js"));
        assert_eq!(x.strength, Some(Strength::Strong));
        assert!(x.is_project());
        assert!(!x.is_candidate());
        assert_eq!(x.status(), "pending");
    }

    #[test]
    fn rust_strong_marker() {
        let x = d(&[("Cargo.toml", false)]);
        assert_eq!(x.language.as_deref(), Some("Rust"));
        assert!(x.is_project());
    }

    #[test]
    fn go_and_zig_markers() {
        assert_eq!(d(&[("go.mod", false)]).language.as_deref(), Some("Go"));
        assert_eq!(d(&[("build.zig", false)]).language.as_deref(), Some("Zig"));
    }

    #[test]
    fn csproj_suffix_is_strong() {
        let x = d(&[("App.csproj", false)]);
        assert_eq!(x.language.as_deref(), Some("C#"));
        assert!(x.is_project()); // 无 git 也是项目
    }

    #[test]
    fn sln_suffix_is_strong() {
        let x = d(&[("MyApp.sln", false)]);
        assert_eq!(x.language.as_deref(), Some("C#"));
        assert!(x.is_project());
    }

    #[test]
    fn python_multi_markers_beat_single_node() {
        // 一个目录同时有 package.json 和两个 Python 标记 → Python 胜
        let x = d(&[
            ("package.json", false),
            ("requirements.txt", false),
            ("pyproject.toml", false),
        ]);
        assert_eq!(x.language.as_deref(), Some("Python"));
    }

    #[test]
    fn weak_makefile_without_git_is_candidate() {
        let x = d(&[("Makefile", false), ("main.c", false)]);
        assert!(!x.is_project());
        assert!(x.is_candidate());
        assert_eq!(x.language.as_deref(), Some("C/C++"));
        assert_eq!(x.status(), "candidate");
    }

    #[test]
    fn weak_makefile_with_git_is_project() {
        let x = d(&[("Makefile", false), (".git", true)]);
        assert!(x.is_project());
        assert!(!x.is_candidate());
        assert_eq!(x.language.as_deref(), Some("C/C++"));
        assert_eq!(x.status(), "pending");
    }

    #[test]
    fn git_only_repo_is_unknown_project() {
        let x = d(&[(".git", true), ("README.md", false)]);
        assert_eq!(x.language.as_deref(), Some(UNKNOWN_LANG));
        assert!(x.is_project());
        assert_eq!(x.strength, None);
        assert_eq!(x.status(), "pending");
    }

    #[test]
    fn plain_dir_is_nothing() {
        let x = d(&[("a.txt", false), ("lib", true)]);
        assert_eq!(x.language, None);
        assert!(!x.is_project());
        assert!(!x.is_candidate());
        assert!(!x.has_git);
    }

    #[test]
    fn git_marker_must_be_a_dir() {
        let x = d(&[(".git", false)]);
        assert!(!x.has_git);
        assert!(!x.is_project());
    }

    #[test]
    fn marker_match_is_case_insensitive() {
        let x = d(&[("PACKAGE.JSON", false)]);
        assert_eq!(x.language.as_deref(), Some("Node.js"));
    }
}
