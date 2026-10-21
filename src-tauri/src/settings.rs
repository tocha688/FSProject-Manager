use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_SHORTCUT: &str = "alt+space";
pub const DEFAULT_EDITOR_CMD: &str = "code";
pub const DEFAULT_MODEL: &str = "glm-4-flash";
pub const DEFAULT_BASE_URL: &str = "https://open.bigmodel.cn/api/paas/v4";
pub const OLLAMA_BASE_URL: &str = "http://localhost:11434/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub roots: Vec<RootConfig>,
    pub ai: AiConfig,
    pub shortcut: String,
    pub editor_cmd: String,
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct RootConfig {
    pub path: String,
    pub depth: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AiConfig {
    /// "openai-compatible"（GLM/OpenAI 等）或 "ollama"
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl Default for RootConfig {
    fn default() -> Self {
        RootConfig {
            path: String::new(),
            depth: 5,
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            provider: "openai-compatible".into(),
            base_url: DEFAULT_BASE_URL.into(),
            api_key: String::new(),
            model: DEFAULT_MODEL.into(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            roots: Vec::new(),
            ai: AiConfig::default(),
            shortcut: DEFAULT_SHORTCUT.into(),
            editor_cmd: DEFAULT_EDITOR_CMD.into(),
            autostart: false,
        }
    }
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

pub fn db_path(dir: &Path) -> PathBuf {
    dir.join("projects.db")
}

pub fn load(dir: &Path) -> Settings {
    match std::fs::read_to_string(config_path(dir)) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save(dir: &Path, s: &Settings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(config_path(dir), json).map_err(|e| e.to_string())
}

/// 保存前的清洗与兜底：裁剪路径、夹紧深度、补默认值。
pub fn normalize(s: &mut Settings) {
    s.roots.retain(|r| !r.path.trim().is_empty());
    for r in &mut s.roots {
        r.path = r.path.trim().to_string();
        r.depth = r.depth.clamp(1, 12);
    }
    s.roots.dedup_by(|a, b| a.path == b.path);
    if s.shortcut.trim().is_empty() {
        s.shortcut = DEFAULT_SHORTCUT.into();
    }
    if s.editor_cmd.trim().is_empty() {
        s.editor_cmd = DEFAULT_EDITOR_CMD.into();
    }
    if s.ai.provider != "openai-compatible" && s.ai.provider != "ollama" {
        s.ai.provider = "openai-compatible".into();
    }
    s.ai.base_url = s.ai.base_url.trim().to_string();
    if s.ai.provider == "ollama" && s.ai.base_url.is_empty() {
        s.ai.base_url = OLLAMA_BASE_URL.into();
    }
    s.ai.model = s.ai.model.trim().to_string();
    if s.ai.model.is_empty() {
        s.ai.model = DEFAULT_MODEL.into();
    }
}

/// AI 是否已配置到可用状态。
pub fn ai_configured(ai: &AiConfig) -> bool {
    if ai.base_url.trim().is_empty() {
        return false;
    }
    match ai.provider.as_str() {
        "ollama" => true,
        _ => !ai.api_key.trim().is_empty(),
    }
}

/// 数据目录：优先环境变量 PF_DATA_DIR（开发/测试用），
/// 其次软件运行目录（便携式），不可写则回退系统 AppData。
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("PF_DATA_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            if dir_writable(p) {
                return p.to_path_buf();
            }
        }
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("project-finder")
}

fn dir_writable(p: &Path) -> bool {
    if std::fs::create_dir_all(p).is_err() {
        return false;
    }
    let probe = p.join(".pf-write-probe");
    match std::fs::write(&probe, b"ok") {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_save_load() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        s.roots.push(RootConfig {
            path: r"C:\Codes".into(),
            depth: 3,
        });
        s.ai.api_key = "sk-test".into();
        save(tmp.path(), &s).unwrap();
        let loaded = load(tmp.path());
        assert_eq!(loaded, s);
    }

    #[test]
    fn missing_file_gives_default() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(tmp.path()), Settings::default());
    }

    #[test]
    fn corrupt_file_gives_default() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(config_path(tmp.path()), "{broken").unwrap();
        assert_eq!(load(tmp.path()), Settings::default());
    }

    #[test]
    fn normalize_clamps_and_cleans() {
        let mut s = Settings::default();
        s.roots.push(RootConfig {
            path: "  C:\\Codes  ".into(),
            depth: 99,
        });
        s.roots.push(RootConfig {
            path: "C:\\Codes".into(),
            depth: 2,
        });
        s.roots.push(RootConfig {
            path: "".into(),
            depth: 1,
        });
        s.shortcut = "  ".into();
        s.editor_cmd = "".into();
        s.ai.provider = "nonsense".into();
        s.ai.model = "".into();
        normalize(&mut s);
        assert_eq!(s.roots.len(), 1); // 空路径剔除、重复路径去重
        assert_eq!(s.roots[0].depth, 12);
        assert_eq!(s.roots[0].path, "C:\\Codes");
        assert_eq!(s.shortcut, DEFAULT_SHORTCUT);
        assert_eq!(s.editor_cmd, DEFAULT_EDITOR_CMD);
        assert_eq!(s.ai.provider, "openai-compatible");
        assert_eq!(s.ai.model, DEFAULT_MODEL);
    }

    #[test]
    fn normalize_fills_ollama_default_url() {
        let mut s = Settings::default();
        s.ai.provider = "ollama".into();
        s.ai.base_url = "".into();
        normalize(&mut s);
        assert_eq!(s.ai.base_url, OLLAMA_BASE_URL);
    }

    #[test]
    fn ai_configured_rules() {
        let mut ai = AiConfig::default();
        assert!(!ai_configured(&ai)); // 有 url 无 key
        ai.api_key = "sk-x".into();
        assert!(ai_configured(&ai));
        ai.provider = "ollama".into();
        ai.api_key = "".into();
        assert!(ai_configured(&ai)); // ollama 不需要 key
        ai.base_url = "".into();
        assert!(!ai_configured(&ai));
    }

    #[test]
    fn data_dir_respects_env_override() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("PF_DATA_DIR", tmp.path());
        let d = data_dir();
        std::env::remove_var("PF_DATA_DIR");
        assert_eq!(d, tmp.path());
    }
}
