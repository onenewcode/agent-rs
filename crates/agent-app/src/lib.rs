use agent_adapters::{OpenRouterModel, ReqwestFetcher, TavilySearchProvider};
use agent_kernel::{
    DocumentParser, Result, Error, ErrorType, RunReport, SearchProvider, SourceFetcher, OrErr,
};
use agent_runtime::AgentOrchestrator;
use docx_domain::{Document, DocxParser, DocumentReviewer, DocumentWriter};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConfig {
    pub openrouter_api_key: String,
    pub tavily_api_key: Option<String>,
    pub writer_model: String,
    pub reviewer_model: String,
    pub max_iterations: usize,
}

impl AppConfig {
    pub fn from_path(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .or_err(ErrorType::Config, "failed to read config file")?;
        toml::from_str(&content)
            .or_err(ErrorType::Config, "failed to parse config as TOML")
    }
}

pub struct AppContainer {
    pub orchestrator: AgentOrchestrator,
    pub parser: DocxParser,
    pub fetcher: Arc<dyn SourceFetcher>,
}

impl AppContainer {
    pub fn from_config(config: &AppConfig) -> Result<Self> {
        let http_client = Arc::new(
            reqwest::Client::builder()
                .build()
                .or_err(ErrorType::Config, "failed to build HTTP client")?,
        );

        let writer_model = Arc::new(OpenRouterModel::new(
            config.writer_model.clone(),
            config.openrouter_api_key.clone(),
        ));

        let reviewer_model = Arc::new(OpenRouterModel::new(
            config.reviewer_model.clone(),
            config.openrouter_api_key.clone(),
        ));

        let search_provider: Arc<dyn SearchProvider> = if let Some(key) = &config.tavily_api_key {
            Arc::new(TavilySearchProvider::new(key.clone(), http_client.clone()))
        } else {
            return Err(Box::new(Error::explain(
                ErrorType::Config,
                "Tavily API key is required for search".to_owned(),
            )));
        };

        let writer = Arc::new(DocumentWriter::new(
            writer_model.clone(),
            search_provider.clone(),
        ));
        let reviewer = Arc::new(DocumentReviewer::new(reviewer_model.clone()));

        let orchestrator =
            AgentOrchestrator::new(writer, reviewer, config.max_iterations);
        let parser = DocxParser::new();
        let fetcher = Arc::new(ReqwestFetcher::new(http_client.clone()));

        Ok(Self {
            orchestrator,
            parser,
            fetcher,
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
