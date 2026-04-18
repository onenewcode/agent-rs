use std::{fs, path::Path};

use serde::Deserialize;
use tracing::info;

use crate::error::DocxAgentError;

pub(crate) const SYSTEM_PROMPT_DEFAULT: &str = r"你是 Word 文档扩写助手。仅基于给定 DOCX、用户要求、URL/搜索材料写作；材料不足时说明假设与边界，不得编造事实或最新数据。输出中文 Markdown，不加引用编号；优先沿用原文主题、术语与结构，必要时补充合理小节。";
pub(crate) const GENERATION_TEMPLATE_DEFAULT: &str = r"任务:
{prompt}

文档:
{document}

用户 URL:
{user_urls}

外部材料:
{sources}

扩写大纲:
{outline}

请基于以上大纲和材料直接输出最终中文 Markdown。";
pub(crate) const OUTLINE_TEMPLATE_DEFAULT: &str = r"任务:
{prompt}

文档:
{document}

用户 URL:
{user_urls}

外部材料:
{sources}

请基于现有文档和外部材料，为扩写任务生成一个详细的中文 Markdown 大纲。";

#[derive(Debug, Clone, Deserialize)]
pub struct DocxAgentConfig {
    pub llm: LlmConfig,
    pub search: SearchConfig,
    pub limits: LimitsConfig,
    pub fetch: FetchConfig,
    #[serde(default)]
    pub search_policy: SearchPolicyConfig,
    pub system_prompt: Option<String>,
    pub generation_template: Option<String>,
    pub outline_template: Option<String>,
    pub max_generation_attempts: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub provider: String,
    pub api_key: Option<String>,
    pub max_results: usize,
    #[serde(default = "SearchConfig::default_timeout_secs")]
    pub timeout_secs: u64,
}

impl SearchConfig {
    fn default_timeout_secs() -> u64 {
        30
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    pub document_tokens: usize,
    pub source_tokens: usize,
    #[serde(default = "LimitsConfig::default_global_timeout_secs")]
    pub global_timeout_secs: u64,
}

impl LimitsConfig {
    fn default_global_timeout_secs() -> u64 {
        180
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FetchConfig {
    pub user_agent: String,
    #[serde(default = "FetchConfig::default_concurrency_limit")]
    pub concurrency_limit: usize,
    #[serde(default = "FetchConfig::default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "FetchConfig::default_cache_dir")]
    pub cache_dir: String,
    #[serde(default = "FetchConfig::default_max_cache_age_days")]
    pub max_cache_age_days: u64,
}

impl FetchConfig {
    fn default_concurrency_limit() -> usize {
        5
    }

    fn default_timeout_secs() -> u64 {
        20
    }

    fn default_cache_dir() -> String {
        ".agent-cache".to_owned()
    }

    fn default_max_cache_age_days() -> u64 {
        7
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchPolicyConfig {
    #[serde(default = "SearchPolicyConfig::default_negations")]
    pub negations: Vec<String>,
    #[serde(default = "SearchPolicyConfig::default_hints")]
    pub hints: Vec<String>,
}

impl Default for SearchPolicyConfig {
    fn default() -> Self {
        Self {
            negations: Self::default_negations(),
            hints: Self::default_hints(),
        }
    }
}

impl SearchPolicyConfig {
    fn default_negations() -> Vec<String> {
        [
            "不要联网",
            "不要搜索",
            "不要检索",
            "无需联网",
            "无需搜索",
            "无需检索",
            "不需要联网",
            "不需要搜索",
            "不需要检索",
            "别联网",
            "别搜索",
            "别检索",
            "do not search",
            "don't search",
            "no search",
            "without search",
            "do not browse",
            "don't browse",
            "no browsing",
            "do not use web",
            "don't use web",
            "no web search",
            "without web search",
            "do not use internet",
            "don't use internet",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn default_hints() -> Vec<String> {
        [
            "搜索", "联网", "最新", "案例", "数据", "资料", "参考", "研究", "趋势", "现状",
            "latest", "current", "search", "research",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    pub(crate) fn should_search(&self, prompt: &str) -> bool {
        let lower = prompt.to_ascii_lowercase();
        if self
            .negations
            .iter()
            .any(|neg| lower.contains(&neg.to_ascii_lowercase()))
        {
            return false;
        }
        self.hints
            .iter()
            .any(|hint| lower.contains(&hint.to_ascii_lowercase()))
    }
}

impl DocxAgentConfig {
    pub fn from_path(path: &Path) -> Result<Self, DocxAgentError> {
        if !path.exists() {
            return Err(DocxAgentError::ConfigNotFound(path.display().to_string()));
        }

        let content = fs::read_to_string(path)?;
        let mut config: Self = toml::from_str(&content)?;
        config.validate()?;
        info!(
            config = %path.display(),
            llm_provider = %config.llm.provider,
            llm_model = %config.llm.model,
            search_provider = %config.search.provider,
            search_enabled = config.search.api_key.is_some(),
            "loaded agent configuration"
        );
        Ok(config)
    }

    pub(crate) fn validate(&mut self) -> Result<(), DocxAgentError> {
        if self.llm.provider != "openrouter" {
            return Err(DocxAgentError::UnsupportedProvider {
                kind: "llm",
                provider: self.llm.provider.clone(),
            });
        }

        if self.search.provider != "tavily" {
            return Err(DocxAgentError::UnsupportedProvider {
                kind: "search",
                provider: self.search.provider.clone(),
            });
        }

        validate_secret("llm.api_key", &self.llm.api_key)?;

        if let Some(api_key) = &self.search.api_key {
            validate_secret("search.api_key", api_key)?;
        }

        Ok(())
    }

    pub(crate) fn system_prompt(&self) -> &str {
        self.system_prompt
            .as_deref()
            .unwrap_or(SYSTEM_PROMPT_DEFAULT)
    }

    pub(crate) fn generation_template(&self) -> &str {
        self.generation_template
            .as_deref()
            .unwrap_or(GENERATION_TEMPLATE_DEFAULT)
    }

    pub(crate) fn outline_template(&self) -> &str {
        self.outline_template
            .as_deref()
            .unwrap_or(OUTLINE_TEMPLATE_DEFAULT)
    }

    pub(crate) fn max_generation_attempts(&self) -> usize {
        self.max_generation_attempts.unwrap_or(3)
    }
}

fn validate_secret(field: &'static str, value: &str) -> Result<(), DocxAgentError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(DocxAgentError::InvalidConfigValue {
            field,
            reason: "value must not be empty",
        });
    }

    if trimmed.starts_with("replace-with-") {
        return Err(DocxAgentError::InvalidConfigValue {
            field,
            reason: "placeholder value must be replaced before running",
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::SearchPolicyConfig;

    #[test]
    fn search_policy_defaults_are_populated() {
        let policy = SearchPolicyConfig::default();
        assert_eq!(policy.negations.len(), 25);
        assert_eq!(policy.hints.len(), 14);
    }

    #[test]
    fn system_prompt_default_is_not_empty() {
        assert!(!super::SYSTEM_PROMPT_DEFAULT.is_empty());
    }

    #[test]
    fn search_policy_uses_prompt_hints() {
        let policy = SearchPolicyConfig::default();
        assert!(policy.should_search("请联网搜索行业最新案例并扩写"));
        assert!(!policy.should_search("请基于文档扩写，不要联网搜索"));
        assert!(!policy.should_search("Please refine this draft, do not search the web."));
        assert!(!policy.should_search("只做语气润色，不要补充事实"));
        assert!(policy.should_search("Please search latest market data and then expand."));
    }

    #[test]
    fn search_policy_is_case_insensitive() {
        let policy = SearchPolicyConfig::default();
        assert!(policy.should_search("LATEST data please"));
        assert!(policy.should_search("SEARCH for more info"));
        assert!(!policy.should_search("DO NOT SEARCH the web"));
        assert!(!policy.should_search("NO SEARCH needed"));
    }
}
