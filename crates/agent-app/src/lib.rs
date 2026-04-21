use agent_adapters::{FileArtifactStore, OpenRouterModel, ReqwestFetcher, TavilySearchProvider};
use agent_kernel::{
    ArtifactStore, DocumentParser, Error, ErrorType, OrErr, Result, RunReport, SearchProvider,
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
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ArtifactsConfig {
    pub dir: String,
}

impl AppConfig {
    /// Loads config from a file path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
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
    /// Creates a new `AppContainer` from the provided config.
    ///
    /// # Errors
    ///
    /// Returns an error if any of the components (HTTP client, models, etc.) fail to initialize.
    pub fn from_config(config: &AppConfig) -> Result<Self> {
        let http_client = Arc::new(
            reqwest::Client::builder()
                .build()
                .or_err(ErrorType::Config, "failed to build HTTP client")?,
        );

        let writer_model = Arc::new(OpenRouterModel::new(
            config.services.models.writer.model.clone(),
            &config.services.models.writer.api_key,
        ));

        let reviewer_model = Arc::new(OpenRouterModel::new(
            config.services.models.reviewer.model.clone(),
            &config.services.models.reviewer.api_key,
        ));

        let search_provider: Arc<dyn SearchProvider> =
            if let Some(key) = &config.services.search.api_key {
                Arc::new(TavilySearchProvider::new(key.clone(), http_client.clone()))
            } else {
                return Err(Box::new(Error::explain(
                    ErrorType::Config,
                    "Tavily API key is required for search".to_owned(),
                )));
            };

        let fetcher = Arc::new(ReqwestFetcher::new(http_client.clone()));

        let writer = Arc::new(DocumentWriter::new(
            writer_model.clone(),
            search_provider.clone(),
            fetcher.clone(),
        ));
        let reviewer = Arc::new(DocumentReviewer::new(reviewer_model.clone()));

        let orchestrator = AgentOrchestrator::new(writer, reviewer, config.runtime.max_iterations);
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

    /// Parses a DOCX document.
    ///
    /// # Errors
    ///
    /// Returns an error if the document cannot be parsed.
    pub fn parse_doc(&self, path: &Path) -> Result<Document> {
        self.parser.parse_path(path)
    }

    /// Runs the document expansion process.
    ///
    /// # Errors
    ///
    /// Returns an error if the expansion process fails.
    pub async fn run_expansion(
        &self,
        task_goal: String,
        initial_doc: String,
    ) -> Result<(RunReport, String)> {
        self.orchestrator.run(task_goal, initial_doc).await
    }
}
