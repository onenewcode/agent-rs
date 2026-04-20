#![allow(clippy::missing_errors_doc)]

use std::{fs, path::Path, sync::Arc};

use agent_adapters::{LlmProviderConfig, TavilySearchProvider, build_openrouter_model};
use agent_kernel::{DocumentParser, LanguageModel, RunError, RunReport, SearchProvider};
use agent_runtime::AgentOrchestrator;
use docx_domain::{DocxDocumentParser, DocxExpandRequest, ReviewerAgent, WriterAgent};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub runtime: RuntimeConfig,
    pub services: ServiceConfig,
    pub docx_expand: DocxWorkflowFileConfig,
}

impl AppConfig {
    pub fn from_path(path: &Path) -> Result<Self, RunError> {
        let content = fs::read_to_string(path)
            .map_err(|error| RunError::Config(format!("failed to read config: {error}")))?;
        toml::from_str::<Self>(&content)
            .map_err(|error| RunError::Config(format!("failed to parse config: {error}")))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    pub default_timeout_secs: u64,
    pub max_iterations: usize,
    pub retry_attempts: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceConfig {
    pub http: HttpConfig,
    pub cache: CacheConfig,
    pub models: std::collections::BTreeMap<String, ModelConfig>,
    pub search: Option<SearchConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpConfig {
    pub user_agent: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CacheConfig {
    pub dir: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    pub model: String,
    pub api_key: String,
    pub input_cost_per_1m: f64,
    pub output_cost_per_1m: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DocxWorkflowFileConfig {
    pub writer_model: String,
    pub reviewer_model: String,
    pub min_score: u8,
    pub document_tokens: usize,
    pub source_tokens: usize,
    pub max_total_tokens: usize,
}

pub struct PlatformApp {
    orchestrator: AgentOrchestrator,
}

impl PlatformApp {
    pub fn from_config(config: &AppConfig) -> Result<Self, RunError> {
        let http = reqwest::Client::builder()
            .user_agent(&config.services.http.user_agent)
            .build()
            .map_err(|error| RunError::Config(format!("failed to build http client: {error}")))?;

        let mut llms = std::collections::BTreeMap::new();
        for (alias, model) in &config.services.models {
            let llm = build_openrouter_model(
                http.clone(),
                LlmProviderConfig {
                    model: model.model.clone(),
                    api_key: model.api_key.clone(),
                    input_cost_per_1m: model.input_cost_per_1m,
                    output_cost_per_1m: model.output_cost_per_1m,
                },
                "You are an autonomous expert AI assistant.".to_owned(),
            )?;
            llms.insert(alias.clone(), Arc::new(llm) as Arc<dyn LanguageModel>);
        }

        let writer_llm = llms
            .get(&config.docx_expand.writer_model)
            .cloned()
            .ok_or_else(|| RunError::Config("writer_model not found".to_owned()))?;
        let reviewer_llm = llms
            .get(&config.docx_expand.reviewer_model)
            .cloned()
            .ok_or_else(|| RunError::Config("reviewer_model not found".to_owned()))?;

        let search_provider = config.services.search.as_ref().map(|s| {
            Arc::new(TavilySearchProvider::new(
                http.clone(),
                &s.api_key,
                8000,
                30,
            )) as Arc<dyn SearchProvider>
        });

        let writer = Arc::new(WriterAgent::new(writer_llm, search_provider));
        let reviewer = Arc::new(ReviewerAgent::new(
            reviewer_llm,
            config.docx_expand.min_score,
        ));

        let mut orchestrator = AgentOrchestrator::new(writer, reviewer, config.runtime.max_iterations);

        if let Some(attempts) = config.runtime.retry_attempts {
            orchestrator = orchestrator.with_retry_policy(agent_runtime::RetryPolicy {
                max_attempts: attempts,
                base_delay_ms: 2000,
            });
        }

        Ok(Self { orchestrator })
    }

    pub fn from_path(path: &Path) -> Result<Self, RunError> {
        Self::from_config(&AppConfig::from_path(path)?)
    }

    pub async fn run_docx(
        &self,
        request: DocxExpandRequest,
    ) -> Result<(RunReport, String), RunError> {
        let parser = DocxDocumentParser;
        let document = parser.parse_path(Path::new(&request.document_path))?;
        self.orchestrator
            .run(request.prompt, document.render_markdown())
            .await
    }
}
