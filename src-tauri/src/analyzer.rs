use crate::settings::AiConfig;
use crate::signals::Signals;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Analysis {
    pub description: String,
    pub tags: Vec<String>,
    /// 固定枚举之一；未识别时为空串。
    pub project_type: String,
}

/// 项目类型固定枚举。前端筛选器与 AI 提示词共用这份定义，
/// 保证 AI 只会输出这些值，避免筛选列表被自由文本撑爆。
pub const PROJECT_TYPES: &[&str] = &[
    "Web 应用",
    "桌面应用",
    "命令行工具",
    "库/框架",
    "爬虫/数据采集",
    "数据分析",
    "自动化脚本",
    "逆向/安全",
    "AI/机器学习",
    "游戏/图形",
    "文档/示例",
    "其他",
];

/// 把模型给出的类型归一到枚举内；无法匹配则返回空串（表示未分类）。
pub fn normalize_type(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    for known in PROJECT_TYPES {
        if known.eq_ignore_ascii_case(t) {
            return (*known).to_string();
        }
    }
    // 宽松匹配：模型可能输出 "Web应用"（无空格）、"web app" 等变体
    let compact = |s: &str| -> String {
        s.chars()
            .filter(|c| !c.is_whitespace() && *c != '/' && *c != '-' && *c != '_')
            .collect::<String>()
            .to_lowercase()
    };
    let c = compact(t);
    for known in PROJECT_TYPES {
        if compact(known) == c {
            return (*known).to_string();
        }
    }
    // 关键词兜底：顺序即优先级，越具体的放前面。
    // 否则 "逆向分析" 会先被 "分析" 吃掉、"web crawler" 会先被 "web" 吃掉。
    let aliases: &[(&str, &str)] = &[
        ("crawler", "爬虫/数据采集"),
        ("spider", "爬虫/数据采集"),
        ("scrap", "爬虫/数据采集"),
        ("爬虫", "爬虫/数据采集"),
        ("采集", "爬虫/数据采集"),
        ("reverse", "逆向/安全"),
        ("逆向", "逆向/安全"),
        ("security", "逆向/安全"),
        ("malware", "逆向/安全"),
        ("安全", "逆向/安全"),
        ("machine learning", "AI/机器学习"),
        ("机器学习", "AI/机器学习"),
        ("神经网络", "AI/机器学习"),
        ("模型", "AI/机器学习"),
        ("automation", "自动化脚本"),
        ("自动化", "自动化脚本"),
        ("script", "自动化脚本"),
        ("脚本", "自动化脚本"),
        ("game", "游戏/图形"),
        ("render", "游戏/图形"),
        ("graphics", "游戏/图形"),
        ("图形", "游戏/图形"),
        ("数据分析", "数据分析"),
        ("分析", "数据分析"),
        ("数据", "数据分析"),
        ("desktop", "桌面应用"),
        ("gui", "桌面应用"),
        ("客户端", "桌面应用"),
        ("cli", "命令行工具"),
        ("command", "命令行工具"),
        ("terminal", "命令行工具"),
        ("命令行", "命令行工具"),
        ("library", "库/框架"),
        ("sdk", "库/框架"),
        ("framework", "库/框架"),
        ("框架", "库/框架"),
        ("库", "库/框架"),
        ("demo", "文档/示例"),
        ("example", "文档/示例"),
        ("示例", "文档/示例"),
        ("blog", "文档/示例"),
        ("文档", "文档/示例"),
        ("web", "Web 应用"),
        ("网站", "Web 应用"),
        ("前端", "Web 应用"),
        ("后端", "Web 应用"),
        ("api", "Web 应用"),
        ("ai", "AI/机器学习"),
        ("data", "数据分析"),
    ];
    let lower = t.to_lowercase();
    for (needle, mapped) in aliases {
        if lower.contains(needle) {
            return (*mapped).to_string();
        }
    }
    String::new()
}

/// 所有分析器实现此接口（可插拔：云端 API / 本地 Ollama / 未来其他）。
pub trait Provider: Send {
    fn name(&self) -> &str;
    fn analyze(&self, name: &str, language: &str, signals: &Signals) -> Result<Analysis, String>;
}

/// OpenAI 兼容聊天接口。GLM、OpenAI、DeepSeek 等云端服务以及
/// Ollama（/v1 路径）都实现了同一协议，因此一个实现覆盖全部。
pub struct OpenAiCompatible {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

pub fn from_config(ai: &AiConfig) -> OpenAiCompatible {
    OpenAiCompatible {
        base_url: ai.base_url.trim().trim_end_matches('/').to_string(),
        api_key: if ai.provider == "ollama" {
            String::new()
        } else {
            ai.api_key.trim().to_string()
        },
        model: ai.model.clone(),
    }
}

pub fn build_prompt(name: &str, language: &str, s: &Signals) -> (String, String) {
    let types = PROJECT_TYPES.join(" / ");
    let system = format!(
        "你是资深软件工程师。用户会给你一个本地代码项目的信号摘要，请判断这个项目是做什么的。\
只输出一个 JSON 对象，格式：{{\"description\":\"不超过50字的中文一句话说明\",\
\"tags\":[\"3到8个标签\"],\"type\":\"项目类型\"}}。\
type 必须从以下取值中选一个完全一致的（不要自创）：{types}。\
不要输出任何其他文字、解释或代码块标记。"
    );

    let mut user = format!("项目名: {name}\n语言: {language}\n");
    if !s.deps.is_empty() {
        user += &format!(
            "依赖: {}\n",
            s.deps.iter().take(40).cloned().collect::<Vec<_>>().join(", ")
        );
    }
    if let Some(r) = &s.readme {
        user += &format!("README片段: {}\n", crate::signals::truncate_chars(r, 600));
    }
    if !s.commits.is_empty() {
        user += &format!(
            "最近提交: {}\n",
            s.commits.iter().take(10).cloned().collect::<Vec<_>>().join(" | ")
        );
    }
    if !s.structure.is_empty() {
        user += &format!("目录结构: {}\n", s.structure.join(", "));
    }
    if let Some(r) = &s.git_remote {
        user += &format!("git remote: {r}\n");
    }
    (system, user)
}

/// 去掉 markdown 代码块围栏行。
fn strip_fences(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 从模型回复中稳健地解析 JSON。
pub fn parse_analysis(text: &str) -> Result<Analysis, String> {
    let cleaned = strip_fences(text);
    let start = cleaned.find('{').ok_or("响应中没有 JSON 对象")?;
    let end = cleaned.rfind('}').ok_or("响应中没有 JSON 对象")?;
    if end < start {
        return Err("JSON 不完整".into());
    }
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        description: String,
        #[serde(default)]
        tags: Vec<String>,
        #[serde(default, alias = "projectType", alias = "project_type")]
        r#type: String,
    }
    let raw: Raw = serde_json::from_str(cleaned[start..=end].trim())
        .map_err(|e| format!("JSON 解析失败: {e}"))?;
    let description = raw.description.trim().to_string();
    if description.is_empty() {
        return Err("描述为空".into());
    }
    let mut tags: Vec<String> = Vec::new();
    for t in raw.tags {
        let t = t.trim().to_string();
        if !t.is_empty() && !tags.contains(&t) {
            tags.push(t);
        }
        if tags.len() >= 10 {
            break;
        }
    }
    Ok(Analysis {
        description: crate::signals::truncate_chars(&description, 200),
        tags,
        project_type: normalize_type(&raw.r#type),
    })
}

impl Provider for OpenAiCompatible {
    fn name(&self) -> &str {
        "openai-compatible"
    }

    fn analyze(&self, name: &str, language: &str, signals: &Signals) -> Result<Analysis, String> {
        if self.base_url.is_empty() {
            return Err("未配置 API 地址".into());
        }
        let (system, user) = build_prompt(name, language, signals);
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .map_err(|e| e.to_string())?;
        let mut req = client
            .post(format!("{}/chat/completions", self.base_url))
            .json(&serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user}
                ],
                "temperature": 0.2
            }));
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }
        let resp = req.send().map_err(|e| format!("请求失败: {e}"))?;
        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .map_err(|e| format!("响应解析失败: {e}"))?;
        if !status.is_success() {
            return Err(format!("API 返回 {status}: {body}"));
        }
        let content = body
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or("响应缺少 choices[0].message.content")?;
        parse_analysis(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_json() {
        let a = parse_analysis(r#"{"description":"一个爬虫","tags":["爬虫","Python"]}"#).unwrap();
        assert_eq!(a.description, "一个爬虫");
        assert_eq!(a.tags, vec!["爬虫", "Python"]);
    }

    #[test]
    fn parse_fenced_json() {
        let text = "好的，以下是分析结果：\n```json\n{\"description\":\"简历解析器\",\"tags\":[\"NLP\"]}\n```\n希望有帮助";
        let a = parse_analysis(text).unwrap();
        assert_eq!(a.description, "简历解析器");
        assert_eq!(a.tags, vec!["NLP"]);
    }

    #[test]
    fn parse_json_with_surrounding_text() {
        let text = r#"分析如下 {"description":"x","tags":[]} 完毕"#;
        assert_eq!(parse_analysis(text).unwrap().description, "x");
    }

    #[test]
    fn rejects_empty_description() {
        assert!(parse_analysis(r#"{"description":"  ","tags":[]}"#).is_err());
        assert!(parse_analysis("完全没有 json").is_err());
    }

    #[test]
    fn tags_are_trimmed_deduped_and_capped() {
        let tags: Vec<String> = (0..20).map(|i| format!("t{i}")).collect();
        let mut dup = vec![" a ".to_string(), "a".to_string(), "".to_string()];
        dup.extend(tags);
        let body = serde_json::json!({ "description": "d", "tags": dup }).to_string();
        let a = parse_analysis(&body).unwrap();
        assert_eq!(a.tags.len(), 10);
        assert_eq!(a.tags[0], "a");
        assert!(a.tags.iter().all(|t| !t.is_empty()));
    }

    #[test]
    fn description_is_truncated() {
        let long = "长".repeat(500);
        let body = serde_json::json!({ "description": long, "tags": [] }).to_string();
        let a = parse_analysis(&body).unwrap();
        assert_eq!(a.description.chars().count(), 201); // 200 + '…'
    }

    #[test]
    fn prompt_contains_all_signal_sections() {
        let s = Signals {
            deps: vec!["express".into()],
            readme: Some("hello readme".into()),
            git_remote: Some("https://example.com/x.git".into()),
            commits: vec!["fix bug".into()],
            last_commit_ts: Some(1),
            structure: vec!["src/".into()],
        };
        let (system, user) = build_prompt("my-app", "Node.js", &s);
        assert!(system.contains("JSON"));
        assert!(user.contains("项目名: my-app"));
        assert!(user.contains("express"));
        assert!(user.contains("hello readme"));
        assert!(user.contains("fix bug"));
        assert!(user.contains("https://example.com/x.git"));
    }

    #[test]
    fn empty_signals_still_produce_prompt() {
        let (_, user) = build_prompt("x", "未知", &Signals::empty());
        assert!(user.contains("项目名: x"));
    }

    #[test]
    fn from_config_ollama_clears_key() {
        let mut ai = AiConfig::default();
        ai.provider = "ollama".into();
        ai.api_key = "should-be-ignored".into();
        ai.base_url = "http://localhost:11434/v1/".into();
        let p = from_config(&ai);
        assert!(p.api_key.is_empty());
        assert_eq!(p.base_url, "http://localhost:11434/v1");
    }

    #[test]
    fn analyze_requires_base_url() {
        let p = OpenAiCompatible {
            base_url: "".into(),
            api_key: "".into(),
            model: "m".into(),
        };
        assert!(p.analyze("x", "未知", &Signals::empty()).is_err());
    }
}

#[cfg(test)]
mod type_tests {
    use super::*;

    #[test]
    fn exact_type_names_are_accepted() {
        for t in PROJECT_TYPES {
            assert_eq!(normalize_type(t), *t, "{t} 应被识别");
        }
    }

    #[test]
    fn whitespace_and_slash_variants_normalize() {
        assert_eq!(normalize_type("Web应用"), "Web 应用");
        assert_eq!(normalize_type("web  应用"), "Web 应用");
        assert_eq!(normalize_type("库框架"), "库/框架");
        assert_eq!(normalize_type("命令行工具"), "命令行工具");
    }

    #[test]
    fn english_words_map_via_keywords() {
        assert_eq!(normalize_type("web app"), "Web 应用");
        assert_eq!(normalize_type("cli tool"), "命令行工具");
        assert_eq!(normalize_type("a small library"), "库/框架");
        assert_eq!(normalize_type("web crawler"), "爬虫/数据采集");
        assert_eq!(normalize_type("desktop gui"), "桌面应用");
    }

    #[test]
    fn chinese_keywords_map() {
        assert_eq!(normalize_type("爬虫"), "爬虫/数据采集");
        assert_eq!(normalize_type("逆向分析工具"), "逆向/安全");
        // 「数据分析」是枚举名本身，精确匹配优先
        assert_eq!(normalize_type("数据分析"), "数据分析");
        assert_eq!(normalize_type("处理数据的工具"), "数据分析");
    }

    #[test]
    fn unknown_type_is_empty() {
        assert_eq!(normalize_type(""), "");
        assert_eq!(normalize_type("这种东西不存在zzz"), "");
    }

    #[test]
    fn parse_analysis_reads_type_field() {
        let a = parse_analysis(
            r#"{"description":"一个爬虫","tags":["爬虫"],"type":"爬虫/数据采集"}"#,
        )
        .unwrap();
        assert_eq!(a.project_type, "爬虫/数据采集");
    }

    #[test]
    fn parse_analysis_accepts_alternative_type_keys() {
        for json in [
            r#"{"description":"d","tags":[],"type":"命令行工具"}"#,
            r#"{"description":"d","tags":[],"projectType":"命令行工具"}"#,
            r#"{"description":"d","tags":[],"project_type":"命令行工具"}"#,
        ] {
            let a = parse_analysis(json).unwrap();
            assert_eq!(a.project_type, "命令行工具", "{json}");
        }
    }

    #[test]
    fn parse_analysis_without_type_yields_empty() {
        let a = parse_analysis(r#"{"description":"d","tags":[]}"#).unwrap();
        assert_eq!(a.project_type, "");
    }

    #[test]
    fn prompt_lists_all_types() {
        let (system, _) = build_prompt("x", "Rust", &Signals::empty());
        for t in PROJECT_TYPES {
            assert!(system.contains(t), "提示词应包含类型 {t}");
        }
    }

    #[test]
    fn normalized_type_is_always_in_enum_or_empty() {
        // 模糊值只能落到枚举内或空，不会产出自由文本
        for raw in ["随便什么", "web", "Web 应用", "unknown stuff123"] {
            let n = normalize_type(raw);
            assert!(n.is_empty() || PROJECT_TYPES.contains(&n.as_str()), "{raw} -> {n}");
        }
    }
}
