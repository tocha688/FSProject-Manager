use crate::settings::AiConfig;
use crate::signals::Signals;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Analysis {
    pub description: String,
    pub tags: Vec<String>,
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
    let system = "你是资深软件工程师。用户会给你一个本地代码项目的信号摘要，请判断这个项目是做什么的。\
只输出一个 JSON 对象，格式：{\"description\":\"不超过50字的中文一句话说明\",\"tags\":[\"3到8个标签\"]}。\
不要输出任何其他文字、解释或代码块标记。"
        .to_string();

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
