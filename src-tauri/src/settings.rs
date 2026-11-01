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
    /// 总开关：关闭后扫描不自动分析，手动分析也会被拒绝。
    pub enabled: bool,
    /// 并发分析数（同时进行的 AI 请求数）。
    pub concurrency: u32,
    /// 最近一次连接测试通过时的配置指纹；配置变更后即失效。
    #[serde(default)]
    pub verified_fingerprint: String,
}

/// 参与"已验证"判定的配置指纹：地址/模型/key/provider 任一变化都需重测。
pub fn ai_fingerprint(ai: &AiConfig) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    ai.provider.hash(&mut h);
    ai.base_url.trim().hash(&mut h);
    ai.model.trim().hash(&mut h);
    ai.api_key.trim().hash(&mut h);
    format!("{:016x}", h.finish())
}

/// 当前配置是否已通过连接测试。
pub fn ai_verified(ai: &AiConfig) -> bool {
    !ai.verified_fingerprint.is_empty() && ai.verified_fingerprint == ai_fingerprint(ai)
}

pub const DEFAULT_CONCURRENCY: u32 = 2;
pub const MAX_CONCURRENCY: u32 = 8;

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
            enabled: false,
            concurrency: DEFAULT_CONCURRENCY,
            verified_fingerprint: String::new(),
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

/// 返回给前端的设置视图：附带"是否已验证"的权威判定，避免前端重复实现指纹逻辑。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    #[serde(flatten)]
    pub settings: Settings,
    pub ai_verified: bool,
}

impl SettingsView {
    pub fn of(s: Settings) -> Self {
        let ai_verified = ai_verified(&s.ai);
        SettingsView {
            settings: s,
            ai_verified,
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
    s.ai.concurrency = s.ai.concurrency.clamp(1, MAX_CONCURRENCY);
    // 配置不完整或未通过连接测试时不允许开启
    if s.ai.enabled && (!ai_configured(&s.ai) || !ai_verified(&s.ai)) {
        s.ai.enabled = false;
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

    #[test]
    fn concurrency_is_clamped() {
        let mut s = Settings::default();
        s.ai.concurrency = 0;
        normalize(&mut s);
        assert_eq!(s.ai.concurrency, 1);
        s.ai.concurrency = 999;
        normalize(&mut s);
        assert_eq!(s.ai.concurrency, MAX_CONCURRENCY);
        s.ai.concurrency = 4;
        normalize(&mut s);
        assert_eq!(s.ai.concurrency, 4);
    }

    #[test]
    fn default_ai_is_disabled() {
        assert!(!AiConfig::default().enabled);
        assert_eq!(AiConfig::default().concurrency, DEFAULT_CONCURRENCY);
    }

    #[test]
    fn enabling_requires_verified_fingerprint() {
        let mut s = Settings::default();
        s.ai.api_key = "sk-x".into();
        s.ai.enabled = true;
        normalize(&mut s);
        // 未测试过 → 不允许开启
        assert!(!s.ai.enabled);
    }

    #[test]
    fn enabling_works_after_verification() {
        let mut s = Settings::default();
        s.ai.api_key = "sk-x".into();
        s.ai.verified_fingerprint = ai_fingerprint(&s.ai);
        assert!(ai_verified(&s.ai));
        s.ai.enabled = true;
        normalize(&mut s);
        assert!(s.ai.enabled);
    }

    #[test]
    fn fingerprint_changes_when_config_changes() {
        let mut ai = AiConfig::default();
        ai.api_key = "k1".into();
        let f1 = ai_fingerprint(&ai);
        ai.api_key = "k2".into();
        assert_ne!(f1, ai_fingerprint(&ai));

        let mut ai2 = AiConfig::default();
        ai2.api_key = "k1".into();
        let f2 = ai_fingerprint(&ai2);
        ai2.model = "other-model".into();
        assert_ne!(f2, ai_fingerprint(&ai2));

        let mut ai3 = AiConfig::default();
        ai3.api_key = "k1".into();
        let f3 = ai_fingerprint(&ai3);
        ai3.base_url = "https://other/v1".into();
        assert_ne!(f3, ai_fingerprint(&ai3));
    }

    #[test]
    fn changing_config_invalidates_verification() {
        let mut ai = AiConfig::default();
        ai.api_key = "k1".into();
        ai.verified_fingerprint = ai_fingerprint(&ai);
        assert!(ai_verified(&ai));
        // 改 key 后，之前的验证失效，开关必须被关掉
        ai.api_key = "k2".into();
        assert!(!ai_verified(&ai));
        let mut s = Settings::default();
        s.ai = ai;
        s.ai.enabled = true;
        normalize(&mut s);
        assert!(!s.ai.enabled);
    }

    #[test]
    fn empty_fingerprint_is_not_verified() {
        let ai = AiConfig::default();
        assert!(!ai_verified(&ai));
    }
}
