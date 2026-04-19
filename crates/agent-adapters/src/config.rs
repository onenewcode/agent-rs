use std::{fs, path::Path};

use agent_kernel::RunError;
use docx_domain::{DocxPromptFormatter, DocxPromptTemplates, TokenBudget};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub runtime: RuntimeConfig,
    pub research: ResearchConfig,
    pub generation: GenerationConfig,
    pub cache: CacheConfig,
    pub observability: ObservabilityConfig,
    pub providers: ProviderConfig,
    pub docx: DocxConfig,
}

impl AppConfig {
    pub fn from_path(path: &Path) -> Result<Self, RunError> {
        if !path.exists() {
            return Err(RunError::Config(format!(
                "config file not found: {}",
                path.display()
            )));
        }

        let content = fs::read_to_string(path)
            .map_err(|error| RunError::Config(format!("failed to read config: {error}")))?;

        let parsed_new = toml::from_str::<NewConfigFile>(&content).map(Self::from_new);
        let parsed_legacy = toml::from_str::<LegacyConfigFile>(&content).map(Self::from_legacy);

        match (parsed_new, parsed_legacy) {
            (Ok(config), _) | (Err(_), Ok(config)) => {
                config.validate()?;
                Ok(config)
            }
            (Err(new_error), Err(legacy_error)) => Err(RunError::Config(format!(
                "failed to parse config as new or legacy format: new={new_error}; legacy={legacy_error}"
            ))),
        }
    }

    #[must_use]
    pub fn prompt_formatter(&self) -> DocxPromptFormatter {
        let defaults = DocxPromptTemplates::default();
        let prompts = DocxPromptTemplates {
            system: self.docx.prompts.system.clone().unwrap_or(defaults.system),
            planning: self
                .docx
                .prompts
                .planning
                .clone()
                .unwrap_or(defaults.planning),
            outline: self
                .docx
                .prompts
                .outline
                .clone()
                .unwrap_or(defaults.outline),
            generation: self
                .docx
                .prompts
                .generation
                .clone()
                .unwrap_or(defaults.generation),
            evaluation: self
                .docx
                .prompts
                .evaluation
                .clone()
                .unwrap_or(defaults.evaluation),
            refinement: self
                .docx
                .prompts
                .refinement
                .clone()
                .unwrap_or(defaults.refinement),
        };

        DocxPromptFormatter::new(
            prompts,
            TokenBudget::new(
                self.generation.document_tokens,
                self.generation.source_tokens,
                self.generation.max_total_tokens,
            ),
        )
    }

    fn validate(&self) -> Result<(), RunError> {
        validate_model("providers.generator", &self.providers.generator)?;
        validate_model("providers.evaluator", &self.providers.evaluator)?;
        if let Some(search) = &self.providers.search {
            validate_secret("providers.search.api_key", &search.api_key)?;
        }
        Ok(())
    }

    fn from_new(value: NewConfigFile) -> Self {
        Self {
            runtime: value.runtime,
            research: value.research,
            generation: value.generation,
            cache: value.cache,
            observability: value.observability,
            providers: value.providers,
            docx: value.docx,
        }
    }

    fn from_legacy(value: LegacyConfigFile) -> Self {
        Self {
            runtime: RuntimeConfig {
                min_score: value.limits.min_score,
                global_timeout_secs: value.limits.global_timeout_secs,
                max_refinement_rounds: value.evaluator.max_attempts.unwrap_or(2),
            },
            research: ResearchConfig {
                max_search_results: value.search.max_results,
                fetch_concurrency_limit: value.fetch.concurrency_limit.unwrap_or(5),
                search_hint_terms: value.search_policy.as_ref().map_or_else(
                    SearchPolicyConfig::default_hints,
                    |policy| {
                        policy
                            .hints
                            .clone()
                            .unwrap_or_else(SearchPolicyConfig::default_hints)
                    },
                ),
                search_negation_terms: value.search_policy.as_ref().map_or_else(
                    SearchPolicyConfig::default_negations,
                    |policy| {
                        policy
                            .negations
                            .clone()
                            .unwrap_or_else(SearchPolicyConfig::default_negations)
                    },
                ),
            },
            generation: GenerationConfig {
                document_tokens: value.limits.document_tokens,
                source_tokens: value.limits.source_tokens,
                max_total_tokens: value.limits.max_total_tokens.unwrap_or(128_000),
            },
            cache: CacheConfig {
                dir: value
                    .fetch
                    .cache_dir
                    .unwrap_or_else(|| ".agent-cache".to_owned()),
                max_age_days: value.fetch.max_cache_age_days.unwrap_or(7),
            },
            observability: ObservabilityConfig {
                user_agent: value.fetch.user_agent,
                fetch_timeout_secs: value.fetch.timeout_secs.unwrap_or(20),
                search_timeout_secs: value.search.timeout_secs.unwrap_or(30),
            },
            providers: ProviderConfig {
                generator: value.generator.llm,
                evaluator: value.evaluator.llm,
                search: Some(SearchConfig {
                    provider: value.search.provider,
                    api_key: value.search.api_key.unwrap_or_default(),
                }),
            },
            docx: DocxConfig {
                prompts: PromptConfig {
                    system: value
                        .generator
                        .prompts
                        .system
                        .or(value.evaluator.prompts.system),
                    planning: None,
                    outline: value.generator.prompts.outline,
                    generation: value.generator.prompts.generation,
                    evaluation: value.evaluator.prompts.evaluation,
                    refinement: value.evaluator.prompts.refinement,
                },
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "RuntimeConfig::default_min_score")]
    pub min_score: u8,
    #[serde(default = "RuntimeConfig::default_global_timeout_secs")]
    pub global_timeout_secs: u64,
    #[serde(default = "RuntimeConfig::default_max_refinement_rounds")]
    pub max_refinement_rounds: usize,
}

impl RuntimeConfig {
    fn default_min_score() -> u8 {
        80
    }

    fn default_global_timeout_secs() -> u64 {
        180
    }

    fn default_max_refinement_rounds() -> usize {
        2
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResearchConfig {
    #[serde(default = "ResearchConfig::default_max_search_results")]
    pub max_search_results: usize,
    #[serde(default = "ResearchConfig::default_fetch_concurrency_limit")]
    pub fetch_concurrency_limit: usize,
    #[serde(default = "SearchPolicyConfig::default_hints")]
    pub search_hint_terms: Vec<String>,
    #[serde(default = "SearchPolicyConfig::default_negations")]
    pub search_negation_terms: Vec<String>,
}

impl ResearchConfig {
    fn default_max_search_results() -> usize {
        5
    }

    fn default_fetch_concurrency_limit() -> usize {
        5
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerationConfig {
    pub document_tokens: usize,
    pub source_tokens: usize,
    #[serde(default = "GenerationConfig::default_max_total_tokens")]
    pub max_total_tokens: usize,
}

impl GenerationConfig {
    fn default_max_total_tokens() -> usize {
        128_000
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "CacheConfig::default_dir")]
    pub dir: String,
    #[serde(default = "CacheConfig::default_max_age_days")]
    pub max_age_days: u64,
}

impl CacheConfig {
    fn default_dir() -> String {
        ".agent-cache".to_owned()
    }

    fn default_max_age_days() -> u64 {
        7
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilityConfig {
    pub user_agent: String,
    #[serde(default = "ObservabilityConfig::default_fetch_timeout_secs")]
    pub fetch_timeout_secs: u64,
    #[serde(default = "ObservabilityConfig::default_search_timeout_secs")]
    pub search_timeout_secs: u64,
}

impl ObservabilityConfig {
    fn default_fetch_timeout_secs() -> u64 {
        20
    }

    fn default_search_timeout_secs() -> u64 {
        30
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub generator: LlmProviderConfig,
    pub evaluator: LlmProviderConfig,
    pub search: Option<SearchConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmProviderConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    #[serde(default = "LlmProviderConfig::default_input_cost")]
    pub input_cost_per_1m: f64,
    #[serde(default = "LlmProviderConfig::default_output_cost")]
    pub output_cost_per_1m: f64,
}

impl LlmProviderConfig {
    fn default_input_cost() -> f64 {
        0.15
    }

    fn default_output_cost() -> f64 {
        0.60
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub provider: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DocxConfig {
    #[serde(default)]
    pub prompts: PromptConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PromptConfig {
    pub system: Option<String>,
    pub planning: Option<String>,
    pub outline: Option<String>,
    pub generation: Option<String>,
    pub evaluation: Option<String>,
    pub refinement: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct NewConfigFile {
    runtime: RuntimeConfig,
    research: ResearchConfig,
    generation: GenerationConfig,
    cache: CacheConfig,
    observability: ObservabilityConfig,
    providers: ProviderConfig,
    #[serde(default)]
    docx: DocxConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyConfigFile {
    generator: LegacyGeneratorConfig,
    evaluator: LegacyEvaluatorConfig,
    search: LegacySearchConfig,
    limits: LegacyLimitsConfig,
    fetch: LegacyFetchConfig,
    search_policy: Option<SearchPolicyConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyGeneratorConfig {
    llm: LlmProviderConfig,
    prompts: LegacyGeneratorPromptsConfig,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyGeneratorPromptsConfig {
    system: Option<String>,
    generation: Option<String>,
    outline: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyEvaluatorConfig {
    llm: LlmProviderConfig,
    prompts: LegacyEvaluatorPromptsConfig,
    max_attempts: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyEvaluatorPromptsConfig {
    system: Option<String>,
    evaluation: Option<String>,
    refinement: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacySearchConfig {
    provider: String,
    api_key: Option<String>,
    max_results: usize,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyLimitsConfig {
    document_tokens: usize,
    source_tokens: usize,
    max_total_tokens: Option<usize>,
    global_timeout_secs: u64,
    min_score: u8,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyFetchConfig {
    user_agent: String,
    concurrency_limit: Option<usize>,
    timeout_secs: Option<u64>,
    cache_dir: Option<String>,
    max_cache_age_days: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchPolicyConfig {
    negations: Option<Vec<String>>,
    hints: Option<Vec<String>>,
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
}

fn validate_model(field: &'static str, value: &LlmProviderConfig) -> Result<(), RunError> {
    if value.provider != "openrouter" {
        return Err(RunError::Config(format!(
            "unsupported provider `{}` for {field}",
            value.provider
        )));
    }

    validate_secret(&format!("{field}.api_key"), &value.api_key)
}

fn validate_secret(field: &str, value: &str) -> Result<(), RunError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RunError::Config(format!("{field} must not be empty")));
    }

    if trimmed.starts_with("replace-with-") {
        return Err(RunError::Config(format!(
            "{field} contains a placeholder secret"
        )));
    }

    Ok(())
}
