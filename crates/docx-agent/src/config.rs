use std::{fs, path::Path};

use serde::Deserialize;
use tracing::info;
use agent_core::config::{LlmConfig, SearchConfig, LimitsConfig};

use crate::error::DocxAgentError;

pub const SYSTEM_PROMPT_DEFAULT: &str = r"你是 Word 文档扩写助手。仅基于给定 DOCX、用户要求、URL/搜索材料写作；材料不足时说明假设与边界，不得编造事实或最新数据。输出中文 Markdown，不加引用编号；优先沿用原文主题、术语与结构，必要时补充合理小节。";
pub const GENERATION_TEMPLATE_DEFAULT: &str = r"任务:
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
pub const OUTLINE_TEMPLATE_DEFAULT: &str = r"任务:
{prompt}

文档:
{document}

用户 URL:
{user_urls}

外部材料:
{sources}

请基于现有文档和外部材料，为扩写任务生成一个详细的中文 Markdown 大纲。";
pub const EVALUATION_TEMPLATE_DEFAULT: &str = r"你是一位严苛的文档评审专家。请对以下扩写内容进行评分。

任务要求:
{prompt}

生成的扩写内容:
{content}

外部参考资料:
{sources}

请基于提供的参考资料核对生成内容的正确性和时效性。

请输出一个 JSON 对象，包含以下字段：
- score: 0 到 100 之间的整数分数。
- reason: 评分理由，包括对准确性、专业性和逻辑性的具体评价。

请仅输出有效的 JSON，不要包含 Markdown 代码块标签或其他文字。";
pub const REFINEMENT_TEMPLATE_DEFAULT: &str = r"任务要求:
{prompt}

之前生成的扩写内容:
{content}

专家评分意见:
{reason}

请根据专家的意见对扩写内容进行优化 and 补充，直接输出最终优化后的中文 Markdown。";

#[derive(Debug, Clone, Deserialize)]
pub struct DocxAgentConfig {
    pub generator: GeneratorConfig,
    pub evaluator: EvaluatorConfig,
    pub search: SearchConfig,
    pub limits: LimitsConfig,
    pub fetch: FetchConfig,
    #[serde(default)]
    pub search_policy: SearchPolicyConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneratorConfig {
    pub llm: LlmConfig,
    pub prompts: GeneratorPromptsConfig,
    pub max_attempts: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneratorPromptsConfig {
    pub system: Option<String>,
    pub generation: Option<String>,
    pub outline: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvaluatorConfig {
    pub llm: LlmConfig,
    pub prompts: EvaluatorPromptsConfig,
    pub max_attempts: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvaluatorPromptsConfig {
    pub system: Option<String>,
    pub evaluation: Option<String>,
    pub refinement: Option<String>,
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

    #[must_use]
    pub fn should_search(&self, prompt: &str) -> bool {
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
            generator_model = %config.generator.llm.model,
            evaluator_model = %config.evaluator.llm.model,
            search_provider = %config.search.provider,
            "loaded agent configuration"
        );
        Ok(config)
    }

    pub(crate) fn validate(&mut self) -> Result<(), DocxAgentError> {
        // Validate Generator LLM
        if self.generator.llm.provider != "openrouter" {
            return Err(DocxAgentError::UnsupportedProvider {
                kind: "generator.llm",
                provider: self.generator.llm.provider.clone(),
            });
        }
        validate_secret("generator.llm.api_key", &self.generator.llm.api_key)?;

        // Validate Evaluator LLM
        if self.evaluator.llm.provider != "openrouter" {
            return Err(DocxAgentError::UnsupportedProvider {
                kind: "evaluator.llm",
                provider: self.evaluator.llm.provider.clone(),
            });
        }
        validate_secret("evaluator.llm.api_key", &self.evaluator.llm.api_key)?;

        if self.search.provider != "tavily" {
            return Err(DocxAgentError::UnsupportedProvider {
                kind: "search",
                provider: self.search.provider.clone(),
            });
        }

        if let Some(api_key) = &self.search.api_key {
            validate_secret("search.api_key", api_key)?;
        }

        Ok(())
    }
}

impl GeneratorConfig {
    pub fn system_prompt(&self) -> &str {
        self.prompts.system.as_deref().unwrap_or(SYSTEM_PROMPT_DEFAULT)
    }

    pub fn generation_template(&self) -> &str {
        self.prompts.generation.as_deref().unwrap_or(GENERATION_TEMPLATE_DEFAULT)
    }

    pub fn outline_template(&self) -> &str {
        self.prompts.outline.as_deref().unwrap_or(OUTLINE_TEMPLATE_DEFAULT)
    }

    pub fn max_attempts(&self) -> usize {
        self.max_attempts.unwrap_or(3)
    }
}

impl EvaluatorConfig {
    pub fn system_prompt(&self) -> &str {
        self.prompts.system.as_deref().unwrap_or(SYSTEM_PROMPT_DEFAULT)
    }

    pub fn evaluation_template(&self) -> &str {
        self.prompts.evaluation.as_deref().unwrap_or(EVALUATION_TEMPLATE_DEFAULT)
    }

    pub fn refinement_template(&self) -> &str {
        self.prompts.refinement.as_deref().unwrap_or(REFINEMENT_TEMPLATE_DEFAULT)
    }

    pub fn max_attempts(&self) -> usize {
        self.max_attempts.unwrap_or(3)
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
