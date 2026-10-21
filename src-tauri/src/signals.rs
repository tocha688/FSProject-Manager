use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct Signals {
    pub deps: Vec<String>,
    pub readme: Option<String>,
    pub git_remote: Option<String>,
    pub commits: Vec<String>,
    pub last_commit_ts: Option<i64>,
    pub structure: Vec<String>,
}

impl Signals {
    pub fn empty() -> Self {
        Signals::default()
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

fn run_git(dir: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    no_window(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 按语言提取项目信号：依赖清单、README 片段、git remote、最近 20 条 commit、目录结构。
pub fn extract(path: &Path, language: &str) -> Signals {
    let deps = extract_deps(path, language);
    let readme = read_readme(path);
    let git_remote = run_git(path, &["remote", "get-url", "origin"])
        .or_else(|| run_git(path, &["config", "--get", "remote.origin.url"]));
    let commits = run_git(path, &["log", "-n", "20", "--pretty=%s"])
        .map(|s| s.lines().map(String::from).collect())
        .unwrap_or_default();
    let last_commit_ts = run_git(path, &["log", "-n", "1", "--pretty=%ct"])
        .and_then(|s| s.trim().parse().ok());
    let structure = dir_structure(path);
    Signals {
        deps,
        readme,
        git_remote,
        commits,
        last_commit_ts,
        structure,
    }
}

fn extract_deps(path: &Path, language: &str) -> Vec<String> {
    match language {
        "Node.js" => path
            .join("package.json")
            .read_to_string()
            .map(|c| parse_package_json(&c).unwrap_or_default())
            .unwrap_or_default(),
        "Python" => {
            let mut out = path
                .join("requirements.txt")
                .read_to_string()
                .map(|c| parse_requirements(&c))
                .unwrap_or_default();
            if let Ok(c) = path.join("pyproject.toml").read_to_string() {
                out.extend(parse_pyproject(&c));
            }
            finish(out)
        }
        "Rust" => path
            .join("Cargo.toml")
            .read_to_string()
            .map(|c| parse_cargo(&c))
            .unwrap_or_default(),
        "Go" => path
            .join("go.mod")
            .read_to_string()
            .map(|c| parse_go_mod(&c))
            .unwrap_or_default(),
        "Java" => path
            .join("pom.xml")
            .read_to_string()
            .map(|c| parse_pom(&c))
            .unwrap_or_default(),
        "C#" => csproj_deps(path),
        _ => Vec::new(),
    }
}

trait ReadToString {
    fn read_to_string(&self) -> std::io::Result<String>;
}
impl ReadToString for Path {
    fn read_to_string(&self) -> std::io::Result<String> {
        std::fs::read_to_string(self)
    }
}

fn finish(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v.truncate(40);
    v
}

fn parse_package_json(content: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for section in ["dependencies", "devDependencies"] {
        if let Some(serde_json::Value::Object(map)) = v.get(section) {
            out.extend(map.keys().cloned());
        }
    }
    Ok(finish(out))
}

fn parse_cargo(content: &str) -> Vec<String> {
    let v: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    v.get("dependencies")
        .and_then(|d| d.as_table())
        .map(|t| finish(t.keys().cloned().collect()))
        .unwrap_or_default()
}

fn parse_go_mod(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in content.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with("//") {
            continue;
        }
        if in_block {
            if l.starts_with(')') {
                in_block = false;
                continue;
            }
            if let Some(first) = l.split_whitespace().next() {
                out.push(first.to_lowercase());
            }
        } else if let Some(rest) = l.strip_prefix("require") {
            let rest = rest.trim();
            if rest.starts_with('(') {
                in_block = true;
            } else if let Some(first) = rest.split_whitespace().next() {
                out.push(first.to_lowercase());
            }
        }
    }
    finish(out)
}

/// "flask>=2.0  # comment" -> "flask"
fn req_name(line: &str) -> Option<String> {
    let l = line.split('#').next()?.trim();
    if l.is_empty() || l.starts_with('-') {
        return None;
    }
    let name: String = l
        .chars()
        .take_while(|c| !matches!(c, '=' | '<' | '>' | '!' | '~' | ';' | '[' | ' ' | '\t'))
        .collect();
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn parse_requirements(content: &str) -> Vec<String> {
    finish(content.lines().filter_map(req_name).collect())
}

fn parse_pyproject(content: &str) -> Vec<String> {
    let v: toml::Value = match toml::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Some(deps) = v
        .get("project")
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_array())
    {
        for dep in deps {
            if let Some(name) = dep.as_str().and_then(req_name) {
                out.push(name);
            }
        }
    }
    if let Some(deps) = v
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_table())
    {
        out.extend(deps.keys().cloned());
    }
    finish(out)
}

/// pom.xml 的 <artifactId> 列表；第一个是项目自身，剔除。
fn parse_pom(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(i) = rest.find("<artifactId>") {
        rest = &rest[i + "<artifactId>".len()..];
        match rest.find("</artifactId>") {
            Some(j) => {
                let name = rest[..j].trim();
                if !name.is_empty() {
                    out.push(name.to_string());
                }
                rest = &rest[j..];
            }
            None => break,
        }
    }
    if out.len() > 1 {
        out.remove(0);
    }
    finish(out)
}

fn parse_csproj(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(i) = rest.find("<PackageReference") {
        rest = &rest[i..];
        match rest.find("Include=\"") {
            Some(j) => {
                let after = &rest[j + "Include=\"".len()..];
                match after.find('"') {
                    Some(k) => {
                        out.push(after[..k].to_string());
                        rest = &after[k..];
                    }
                    None => break,
                }
            }
            None => break,
        }
    }
    finish(out)
}

fn csproj_deps(path: &Path) -> Vec<String> {
    let rd = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".csproj") {
            if let Ok(c) = std::fs::read_to_string(e.path()) {
                out.extend(parse_csproj(&c));
            }
        }
    }
    finish(out)
}

fn read_readme(path: &Path) -> Option<String> {
    for name in ["README.md", "readme.md", "Readme.md", "README.txt", "README"] {
        if let Ok(c) = std::fs::read_to_string(path.join(name)) {
            let trimmed = c.trim();
            if !trimmed.is_empty() {
                return Some(truncate_chars(trimmed, 1200));
            }
        }
    }
    None
}

/// 按字符数截断（多字节安全）。
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// 两层目录结构（根条目 + 每个子目录的一级子项），给 AI 当地图用。
fn dir_structure(path: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let entries = match crate::scan::dir_entries(path) {
        Some(e) => e,
        None => return out,
    };
    for (name, is_dir) in entries.iter().take(30) {
        out.push(if *is_dir {
            format!("{name}/")
        } else {
            name.clone()
        });
    }
    for (name, _) in entries
        .iter()
        .filter(|(n, d)| *d && !crate::scan::is_skipped(n))
        .take(8)
    {
        if out.len() >= 60 {
            break;
        }
        if let Some(children) = crate::scan::dir_entries(&path.join(name)) {
            for (child, child_is_dir) in children.iter().take(10) {
                let suffix = if *child_is_dir { "/" } else { "" };
                out.push(format!("{name}/{child}{suffix}"));
            }
        }
    }
    out.truncate(60);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, content: &str) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn parses_package_json() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("package.json"),
            r#"{"name":"x","dependencies":{"express":"^4","left-pad":"1"},"devDependencies":{"vitest":"2"}}"#,
        );
        let s = extract(tmp.path(), "Node.js");
        assert_eq!(s.deps, vec!["express", "left-pad", "vitest"]);
    }

    #[test]
    fn parses_broken_package_json_without_panic() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("package.json"), "{not json");
        let s = extract(tmp.path(), "Node.js");
        assert!(s.deps.is_empty());
    }

    #[test]
    fn parses_cargo_toml() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("Cargo.toml"),
            "[package]\nname=\"x\"\n\n[dependencies]\nserde={version=\"1\"}\nreqwest=\"0.12\"\n",
        );
        let s = extract(tmp.path(), "Rust");
        assert_eq!(s.deps, vec!["reqwest", "serde"]);
    }

    #[test]
    fn parses_go_mod_block_and_inline() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("go.mod"),
            "module x\n\ngo 1.21\n\nrequire (\n\tgithub.com/foo/bar v1.0.0\n\tgolang.org/x/text v0.1 // indirect\n)\n\nrequire github.com/single/one v2.0.0\n",
        );
        let s = extract(tmp.path(), "Go");
        assert_eq!(
            s.deps,
            vec![
                "github.com/foo/bar",
                "github.com/single/one",
                "golang.org/x/text"
            ]
        );
    }

    #[test]
    fn parses_requirements_txt() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("requirements.txt"),
            "flask>=2.0\nrequests==2.31.0\n# comment\n-e .\nDjango~=5.0\n",
        );
        let s = extract(tmp.path(), "Python");
        assert_eq!(s.deps, vec!["django", "flask", "requests"]);
    }

    #[test]
    fn parses_pyproject_toml() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("pyproject.toml"),
            "[project]\ndependencies=[\"click>=8\", \"rich\"]\n\n[tool.poetry.dependencies]\npython=\"^3.11\"\nhttpx=\"*\"\n",
        );
        let s = extract(tmp.path(), "Python");
        assert_eq!(s.deps, vec!["click", "httpx", "python", "rich"]);
    }

    #[test]
    fn parses_pom_xml() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("pom.xml"),
            "<project><artifactId>my-app</artifactId>\
             <dependencies><dependency><artifactId>junit</artifactId></dependency>\
             <dependency><artifactId>log4j</artifactId></dependency></dependencies></project>",
        );
        let s = extract(tmp.path(), "Java");
        assert_eq!(s.deps, vec!["junit", "log4j"]);
    }

    #[test]
    fn parses_csproj() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("App.csproj"),
            "<Project><ItemGroup>\
             <PackageReference Include=\"Newtonsoft.Json\" Version=\"13\" />\
             <PackageReference Include=\"Serilog\" Version=\"4\" />\
             </ItemGroup></Project>",
        );
        let s = extract(tmp.path(), "C#");
        assert_eq!(s.deps, vec!["Newtonsoft.Json", "Serilog"]);
    }

    #[test]
    fn unknown_language_has_no_deps() {
        let tmp = tempfile::tempdir().unwrap();
        let s = extract(tmp.path(), "未知");
        assert!(s.deps.is_empty());
    }

    #[test]
    fn reads_and_truncates_readme() {
        let tmp = tempfile::tempdir().unwrap();
        let long = "好".repeat(2000);
        write(&tmp.path().join("README.md"), &long);
        let s = extract(tmp.path(), "未知");
        let r = s.readme.unwrap();
        assert_eq!(r.chars().count(), 1201); // 1200 + '…'
    }

    #[test]
    fn collects_dir_structure_two_levels() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("src/main.rs"), "");
        write(&tmp.path().join("src/lib.rs"), "");
        write(&tmp.path().join("README.md"), "");
        let s = extract(tmp.path(), "Rust");
        assert!(s.structure.contains(&"src/".to_string()));
        assert!(s.structure.contains(&"src/main.rs".to_string()));
        assert!(s.structure.contains(&"README.md".to_string()));
        assert!(!s.structure.iter().any(|e| e.contains("node_modules")));
    }

    #[test]
    fn reads_git_info_from_real_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let git = |args: &[&str]| {
            let mut c = Command::new("git");
            c.arg("-C").arg(root).args(args);
            let out = c.output().expect("git 可用");
            assert!(out.status.success(), "git {args:?} 失败");
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t.t"]);
        git(&["config", "user.name", "t"]);
        git(&["commit", "--allow-empty", "-q", "-m", "init commit"]);
        git(&["remote", "add", "origin", "https://example.com/x.git"]);

        let s = extract(root, "未知");
        assert_eq!(s.commits, vec!["init commit"]);
        assert!(s.last_commit_ts.is_some());
        assert_eq!(s.git_remote.as_deref(), Some("https://example.com/x.git"));
    }

    #[test]
    fn no_git_dir_gives_no_git_signals() {
        let tmp = tempfile::tempdir().unwrap();
        let s = extract(tmp.path(), "Node.js");
        assert!(s.commits.is_empty());
        assert!(s.last_commit_ts.is_none());
        assert!(s.git_remote.is_none());
    }

    #[test]
    fn truncate_chars_is_unicode_safe() {
        assert_eq!(truncate_chars("中文测试", 2), "中文…");
        assert_eq!(truncate_chars("ab", 5), "ab");
    }
}
