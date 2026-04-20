#![allow(clippy::missing_errors_doc)]

use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

use agent_adapters::{
    DiskCacheSourceFetcher, JsonFileArtifactStore, LlmProviderConfig, TavilySearchProvider,
    WebPageSourceFetcher, build_openrouter_model,
};
use agent_kernel::{
    ArtifactStore, CapabilityRegistry, LanguageModel, RunError, RunOptions, RunReport, RunRequest,
    SearchProvider, SourceFetcher,
};
use agent_runtime::{ExecutorSettings, WorkflowExecutor};
use docx_domain::{
    DocxExpandRequest, DocxFinalOutput, DocxPromptTemplates, DocxWorkflow, DocxWorkflowConfig,
    TokenBudget,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub runtime: RuntimeConfig,
    pub services: ServiceConfig,
    pub workflows: WorkflowConfig,
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
        let config = toml::from_str::<Self>(&content)
            .map_err(|error| RunError::Config(format!("failed to parse config: {error}")))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), RunError> {
        if self.services.models.is_empty() {
            return Err(RunError::Config(
                "at least one model must be configured under [services.models]".to_owned(),
            ));
        }

        validate_alias(
            "workflows.docx_expand.writer_model",
            &self.workflows.docx_expand.writer_model,
            &self.services.models,
        )?;
        validate_alias(
            "workflows.docx_expand.reviewer_model",
            &self.workflows.docx_expand.reviewer_model,
            &self.services.models,
        )?;

        if let Some(alias) = &self.workflows.docx_expand.planner_model {
            validate_alias(
                "workflows.docx_expand.planner_model",
                alias,
                &self.services.models,
            )?;
        }

        for (name, model) in &self.services.models {
            validate_secret(&format!("services.models.{name}.api_key"), &model.api_key)?;
        }

        if let Some(search) = &self.services.search {
            validate_secret("services.search.api_key", &search.api_key)?;
        }

        Ok(())
    }
}

fn validate_alias(
    field: &str,
    alias: &str,
    models: &BTreeMap<String, ModelConfig>,
) -> Result<(), RunError> {
    if models.contains_key(alias) {
        Ok(())
    } else {
        Err(RunError::Config(format!(
            "{field} references unknown model alias `{alias}`"
        )))
    }
}

fn validate_secret(field: &str, value: &str) -> Result<(), RunError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RunError::Config(format!("{field} must not be empty")));
    }
    if trimmed.starts_with("replace-with-") {
        return Err(RunError::Config(format!(
            "{field} still contains a placeholder secret"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "RuntimeConfig::default_timeout")]
    pub default_timeout_secs: u64,
    #[serde(default = "RuntimeConfig::default_capture_artifacts")]
    pub capture_artifacts: bool,
}

impl RuntimeConfig {
    fn default_timeout() -> u64 {
        180
    }

    fn default_capture_artifacts() -> bool {
        true
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceConfig {
    pub http: HttpConfig,
    pub cache: CacheConfig,
    #[serde(default)]
    pub artifacts: ArtifactStoreConfig,
    pub models: BTreeMap<String, ModelConfig>,
    pub search: Option<SearchConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpConfig {
    pub user_agent: String,
    #[serde(default = "HttpConfig::default_fetch_timeout_secs")]
    pub fetch_timeout_secs: u64,
    #[serde(default = "HttpConfig::default_search_timeout_secs")]
    pub search_timeout_secs: u64,
}

impl HttpConfig {
    fn default_fetch_timeout_secs() -> u64 {
        20
    }

    fn default_search_timeout_secs() -> u64 {
        30
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
pub struct ArtifactStoreConfig {
    #[serde(default = "ArtifactStoreConfig::default_dir")]
    pub dir: String,
    #[serde(default = "ArtifactStoreConfig::default_persist")]
    pub persist_reports: bool,
}

impl Default for ArtifactStoreConfig {
    fn default() -> Self {
        Self {
            dir: Self::default_dir(),
            persist_reports: Self::default_persist(),
        }
    }
}

impl ArtifactStoreConfig {
    fn default_dir() -> String {
        ".agent-cache/runs".to_owned()
    }

    fn default_persist() -> bool {
        true
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    #[serde(default = "ModelConfig::default_input_cost")]
    pub input_cost_per_1m: f64,
    #[serde(default = "ModelConfig::default_output_cost")]
    pub output_cost_per_1m: f64,
}

impl ModelConfig {
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

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowConfig {
    #[serde(rename = "docx_expand")]
    pub docx_expand: DocxWorkflowFileConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DocxWorkflowFileConfig {
    pub writer_model: String,
    pub reviewer_model: String,
    pub planner_model: Option<String>,
    #[serde(default = "DocxWorkflowFileConfig::default_min_score")]
    pub min_score: u8,
    #[serde(default = "DocxWorkflowFileConfig::default_max_refinement_rounds")]
    pub max_refinement_rounds: usize,
    #[serde(default = "DocxWorkflowFileConfig::default_search_max_results")]
    pub search_max_results: usize,
    #[serde(default = "DocxWorkflowFileConfig::default_fetch_concurrency_limit")]
    pub fetch_concurrency_limit: usize,
    pub document_tokens: usize,
    pub source_tokens: usize,
    #[serde(default = "DocxWorkflowFileConfig::default_max_total_tokens")]
    pub max_total_tokens: usize,
    #[serde(default = "DocxWorkflowFileConfig::default_search_hint_terms")]
    pub search_hint_terms: Vec<String>,
    #[serde(default = "DocxWorkflowFileConfig::default_search_negation_terms")]
    pub search_negation_terms: Vec<String>,
    #[serde(default)]
    pub prompts: PromptOverrides,
}

impl DocxWorkflowFileConfig {
    fn default_min_score() -> u8 {
        80
    }

    fn default_max_refinement_rounds() -> usize {
        2
    }

    fn default_search_max_results() -> usize {
        5
    }

    fn default_fetch_concurrency_limit() -> usize {
        5
    }

    fn default_max_total_tokens() -> usize {
        128_000
    }

    fn default_search_hint_terms() -> Vec<String> {
        [
            "搜索", "联网", "最新", "案例", "数据", "资料", "参考", "研究", "趋势", "现状",
            "latest", "current", "search", "research",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn default_search_negation_terms() -> Vec<String> {
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
            "do not search",
            "don't search",
            "do not browse",
            "don't browse",
            "do not use web",
            "don't use web",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PromptOverrides {
    pub system: Option<String>,
    pub planning: Option<String>,
    pub outline: Option<String>,
    pub generation: Option<String>,
    pub evaluation: Option<String>,
    pub refinement: Option<String>,
}

pub struct PlatformApp {
    executor: WorkflowExecutor,
    runtime: RuntimeConfig,
}

impl PlatformApp {
    pub fn from_config(config: AppConfig) -> Result<Self, RunError> {
        let http = reqwest::Client::builder()
            .user_agent(&config.services.http.user_agent)
            .build()
            .map_err(|error| RunError::Config(format!("failed to build http client: {error}")))?;

        let llms = config
            .services
            .models
            .iter()
            .map(|(alias, model)| {
                if model.provider != "openrouter" {
                    return Err(RunError::Config(format!(
                        "unsupported LLM provider `{}` for alias `{alias}`",
                        model.provider
                    )));
                }

                let llm = build_openrouter_model(
                    http.clone(),
                    LlmProviderConfig {
                        model: model.model.clone(),
                        api_key: model.api_key.clone(),
                        input_cost_per_1m: model.input_cost_per_1m,
                        output_cost_per_1m: model.output_cost_per_1m,
                    },
                    merged_docx_templates(&config.workflows.docx_expand).system,
                )?;

                Ok((alias.clone(), Arc::new(llm) as Arc<dyn LanguageModel>))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        let docx_workflow_config = build_docx_workflow_config(&config.workflows.docx_expand);
        let source_fetcher = Arc::new(DiskCacheSourceFetcher::new(
            WebPageSourceFetcher::new(
                http.clone(),
                docx_workflow_config.token_budget.source_tokens * 4,
                config.services.http.fetch_timeout_secs,
            ),
            &config.services.cache.dir,
            config.services.cache.max_age_days,
        )) as Arc<dyn SourceFetcher>;

        let search_provider =
            build_search_provider(config.services.search.as_ref(), &config.services.http, &http)?;
        let artifact_store = if config.services.artifacts.persist_reports {
            Some(Arc::new(JsonFileArtifactStore::new(&config.services.artifacts.dir))
                as Arc<dyn ArtifactStore>)
        } else {
            None
        };

        let services = Arc::new(AppServices {
            llms,
            source_fetcher,
            search_provider,
            artifact_store,
        });

        let executor = WorkflowExecutor::builder(
            services,
            ExecutorSettings {
                default_timeout_secs: config.runtime.default_timeout_secs,
                capture_artifacts: config.runtime.capture_artifacts,
            },
        )
        .register_workflow(Arc::new(DocxWorkflow::new(docx_workflow_config)))
        .build();

        Ok(Self {
            executor,
            runtime: config.runtime,
        })
    }

    pub fn from_path(path: &Path) -> Result<Self, RunError> {
        Self::from_config(AppConfig::from_path(path)?)
    }

    pub async fn run_docx(&self, request: DocxExpandRequest) -> Result<RunReport, RunError> {
        self.executor
            .run(RunRequest {
                workflow: "docx.expand".to_owned(),
                input: serde_json::to_value(request).map_err(|error| {
                    RunError::Workflow(format!("failed to serialize docx workflow request: {error}"))
                })?,
                options: RunOptions::with_defaults(
                    self.runtime.default_timeout_secs,
                    self.runtime.capture_artifacts,
                ),
            })
            .await
    }
}

pub fn decode_docx_output(report: &RunReport) -> Result<DocxFinalOutput, RunError> {
    report.output()
}

fn merged_docx_templates(config: &DocxWorkflowFileConfig) -> DocxPromptTemplates {
    let defaults = DocxPromptTemplates::default();
    DocxPromptTemplates {
        system: config.prompts.system.clone().unwrap_or(defaults.system),
        planning: config.prompts.planning.clone().unwrap_or(defaults.planning),
        outline: config.prompts.outline.clone().unwrap_or(defaults.outline),
        generation: config
            .prompts
            .generation
            .clone()
            .unwrap_or(defaults.generation),
        evaluation: config
            .prompts
            .evaluation
            .clone()
            .unwrap_or(defaults.evaluation),
        refinement: config
            .prompts
            .refinement
            .clone()
            .unwrap_or(defaults.refinement),
    }
}

fn build_docx_workflow_config(config: &DocxWorkflowFileConfig) -> DocxWorkflowConfig {
    DocxWorkflowConfig {
        planner_model: config.planner_model.clone(),
        writer_model: config.writer_model.clone(),
        reviewer_model: config.reviewer_model.clone(),
        min_score: config.min_score,
        max_refinement_rounds: config.max_refinement_rounds,
        search_max_results: config.search_max_results,
        fetch_concurrency_limit: config.fetch_concurrency_limit,
        search_hint_terms: config.search_hint_terms.clone(),
        search_negation_terms: config.search_negation_terms.clone(),
        prompt_templates: merged_docx_templates(config),
        token_budget: TokenBudget::new(
            config.document_tokens,
            config.source_tokens,
            config.max_total_tokens,
        ),
    }
}

fn build_search_provider(
    config: Option<&SearchConfig>,
    http_config: &HttpConfig,
    http: &reqwest::Client,
) -> Result<Option<Arc<dyn SearchProvider>>, RunError> {
    let Some(search) = config else {
        return Ok(None);
    };

    if search.provider != "tavily" {
        return Err(RunError::Config(format!(
            "unsupported search provider `{}`",
            search.provider
        )));
    }

    Ok(Some(Arc::new(TavilySearchProvider::new(
        http.clone(),
        &search.api_key,
        8_000,
        http_config.search_timeout_secs,
    )) as Arc<dyn SearchProvider>))
}

struct AppServices {
    llms: BTreeMap<String, Arc<dyn LanguageModel>>,
    source_fetcher: Arc<dyn SourceFetcher>,
    search_provider: Option<Arc<dyn SearchProvider>>,
    artifact_store: Option<Arc<dyn ArtifactStore>>,
}

impl CapabilityRegistry for AppServices {
    fn llm(&self, name: &str) -> Result<Arc<dyn LanguageModel>, RunError> {
        self.llms.get(name).cloned().ok_or_else(|| {
            RunError::Workflow(format!("LLM capability `{name}` is not registered"))
        })
    }

    fn source_fetcher(&self) -> Result<Arc<dyn SourceFetcher>, RunError> {
        Ok(self.source_fetcher.clone())
    }

    fn search_provider(&self) -> Option<Arc<dyn SearchProvider>> {
        self.search_provider.clone()
    }

    fn artifact_store(&self) -> Option<Arc<dyn ArtifactStore>> {
        self.artifact_store.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn parses_new_canonical_config() {
        let config = toml::from_str::<AppConfig>(
            r#"
            [runtime]
            default_timeout_secs = 90
            capture_artifacts = true

            [services.http]
            user_agent = "agent-rs"

            [services.cache]
            dir = ".agent-cache"

            [services.artifacts]
            dir = ".agent-cache/runs"
            persist_reports = true

            [services.models.writer]
            provider = "openrouter"
            model = "model-a"
            api_key = "secret"

            [services.models.reviewer]
            provider = "openrouter"
            model = "model-b"
            api_key = "secret"

            [workflows.docx_expand]
            writer_model = "writer"
            reviewer_model = "reviewer"
            document_tokens = 4000
            source_tokens = 2000
            "#,
        )
        .expect("config should parse");

        assert_eq!(config.workflows.docx_expand.writer_model, "writer");
    }
}
