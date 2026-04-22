use agent_adapters::{FileArtifactStore, OpenRouterModel, ReqwestFetcher, TavilySearchProvider};
use agent_kernel::{
    AgentError, ArtifactStore, DocumentParser, ErrorType, OrErr, Result, RunReport, SearchProvider,
    SourceFetcher,
};
use agent_runtime::AgentOrchestrator;
use docx_domain::{Document, DocumentReviewer, DocumentWriter, DocxParser};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConfig {
    pub runtime: RuntimeConfig,
    pub services: ServicesConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RuntimeConfig {
    pub max_iterations: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ServicesConfig {
    pub models: ModelsConfig,
    pub search: SearchConfig,
    pub artifacts: ArtifactsConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModelsConfig {
    pub writer: ModelProviderConfig,
    pub reviewer: ModelProviderConfig,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ModelProviderConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SearchConfig {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ArtifactsConfig {
    pub dir: String,
}

impl AppConfig {
    pub fn from_path(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .or_err(ErrorType::Config, "failed to read config file")?;
        toml::from_str(&content).or_err(ErrorType::Config, "failed to parse config as TOML")
    }
}

pub struct AppContainer {
    pub orchestrator: AgentOrchestrator,
    pub parser: DocxParser,
    pub fetcher: Arc<dyn SourceFetcher>,
    pub storage: Arc<dyn ArtifactStore>,
}

impl AppContainer {
    pub fn from_config(config: &AppConfig) -> Result<Self> {
        let http_client = Arc::new(
            reqwest::Client::builder()
                .build()
                .or_err(ErrorType::Config, "failed to build HTTP client")?,
        );

        let writer_model = Arc::new(OpenRouterModel::new(
            config.services.models.writer.model.clone(),
            &config.services.models.writer.api_key,
        )?);

        let reviewer_model = Arc::new(OpenRouterModel::new(
            config.services.models.reviewer.model.clone(),
            &config.services.models.reviewer.api_key,
        )?);

        let search_provider: Arc<dyn SearchProvider> =
            if let Some(key) = &config.services.search.api_key {
                Arc::new(TavilySearchProvider::new(
                    key.clone(),
                    http_client.clone(),
                    config.services.search.base_url.clone(),
                ))
            } else {
                return Err(AgentError::explain(
                    ErrorType::Config,
                    "Tavily API key is required for search",
                ));
            };

        let fetcher = Arc::new(ReqwestFetcher::new(http_client.clone()));

        let writer = Arc::new(DocumentWriter::new(
            writer_model.clone(),
            search_provider.clone(),
            fetcher.clone(),
        ));
        let reviewer = Arc::new(DocumentReviewer::new(reviewer_model.clone()));
        let auditor = Arc::new(agent_runtime::DialogueInspector::new());

        let orchestrator =
            AgentOrchestrator::new(writer, reviewer, auditor, config.runtime.max_iterations);
        let parser = DocxParser::new();
        let storage = Arc::new(FileArtifactStore::new(PathBuf::from(
            &config.services.artifacts.dir,
        )));

        Ok(Self {
            orchestrator,
            parser,
            fetcher,
            storage,
        })
    }

    pub fn parse_doc(&self, path: &Path) -> Result<Document> {
        self.parser.parse_path(path)
    }

    pub async fn run_expansion(
        &self,
        task_goal: String,
        initial_doc: String,
    ) -> Result<(RunReport, String)> {
        self.orchestrator.run(task_goal, initial_doc).await
    }
}
