use std::{path::Path, sync::Arc, time::Duration};

use agent_core::{ExpansionRequest, ExpansionResult, ExpansionRuntime, FetchedSource};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::{info, warn};

use crate::{
    config::DocxAgentConfig,
    domain,
    error::DocxAgentError,
    infrastructure::{
        docx::DocxDocumentParser, fetch::WebPageFetcher, llm, search::TavilySearchClient,
    },
};

pub struct DocxExpansionService {
    config: DocxAgentConfig,
    http: reqwest::Client,
    search_client: Option<TavilySearchClient>,
}

impl DocxExpansionService {
    pub fn from_config_path(path: &Path) -> Result<Self, DocxAgentError> {
        let config = DocxAgentConfig::from_path(path)?;
        Self::from_config(config)
    }

    pub fn from_config(config: DocxAgentConfig) -> Result<Self, DocxAgentError> {
        let http = reqwest::Client::builder()
            .user_agent(&config.fetch.user_agent)
            .build()?;

        let search_client = config.search.api_key.as_deref().map(|api_key| {
            TavilySearchClient::new(http.clone(), api_key, config.limits.source_chars)
        });

        info!(
            search_enabled = search_client.is_some(),
            source_char_limit = config.limits.source_chars,
            document_char_limit = config.limits.document_chars,
            "initialized docx expansion service"
        );

        Ok(Self {
            config,
            http,
            search_client,
        })
    }

    pub fn parse_document(
        &self,
        path: &Path,
    ) -> Result<agent_core::ParsedDocument, DocxAgentError> {
        DocxDocumentParser::parse(path)
    }

    pub async fn expand_file(
        &self,
        path: &Path,
        prompt: &str,
        user_urls: &[String],
    ) -> Result<ExpansionResult, DocxAgentError> {
        info!(
            doc = %path.display(),
            prompt_chars = prompt.chars().count(),
            user_urls = user_urls.len(),
            "starting expansion request"
        );
        let document = self.parse_document(path)?;
        self.expand(ExpansionRequest {
            prompt: prompt.to_owned(),
            document,
            user_urls: user_urls.to_vec(),
        })
        .await
        .map_err(Into::into)
    }

    async fn collect_user_sources(
        &self,
        urls: &[String],
    ) -> Result<Vec<FetchedSource>, DocxAgentError> {
        if urls.is_empty() {
            return Ok(Vec::new());
        }

        let fetcher = WebPageFetcher::new(self.http.clone(), self.config.limits.source_chars);
        let semaphore = Arc::new(Semaphore::new(self.config.fetch.concurrency_limit));
        let mut set = tokio::task::JoinSet::new();
        let timeout_dur = Duration::from_secs(self.config.fetch.timeout_secs);

        for url in urls {
            let f = fetcher.clone();
            let u = url.clone();
            let sem = Arc::clone(&semaphore);
            set.spawn(async move {
                let _permit = sem.acquire().await.map_err(|e| DocxAgentError::Agent(Box::new(e)))?;
                match timeout(timeout_dur, f.fetch_url(&u)).await {
                    Ok(res) => res,
                    Err(_) => Err(DocxAgentError::ResearchError {
                        kind: "fetch_timeout",
                        message: format!("fetching {u} timed out after {}s", timeout_dur.as_secs()),
                    }),
                }
            });
        }

        let mut sources = Vec::with_capacity(urls.len());
        while let Some(res) = set.join_next().await {
            match res {
                Ok(Ok(source)) => sources.push(source),
                Ok(Err(e)) => {
                    warn!(error = %e, "failed to fetch user URL, skipping");
                }
                Err(e) => {
                    warn!(error = %e, "join error during URL fetch");
                }
            }
        }
        Ok(sources)
    }

    async fn collect_search_sources(
        &self,
        request: &ExpansionRequest,
    ) -> Result<Option<(String, Vec<FetchedSource>)>, DocxAgentError> {
        let search_requested = self.config.search_policy.should_search(&request.prompt);
        info!(
            search_requested,
            search_enabled = self.search_client.is_some(),
            user_urls = request.user_urls.len(),
            "evaluated external research policy"
        );

        if !search_requested {
            return Ok(None);
        }

        let query = domain::build_search_query(request);
        let Some(backend) = self.search_client.as_ref() else {
            tracing::warn!("prompt requested search but no search API key is configured");
            return Ok(None);
        };

        let timeout_dur = Duration::from_secs(self.config.search.timeout_secs);
        let results = match timeout(timeout_dur, backend.search(&query, self.config.search.max_results)).await {
            Ok(res) => res?,
            Err(_) => {
                warn!(query, "search timed out after {}s", timeout_dur.as_secs());
                return Ok(None);
            }
        };

        if results.is_empty() {
            return Ok(None);
        }
        Ok(Some((query, results)))
    }

    async fn generate_content(
        &self,
        request: &ExpansionRequest,
        sources: &[FetchedSource],
    ) -> Result<String, DocxAgentError> {
        let agent = llm::build_agent(&self.http, &self.config)?;
        let prompt_text =
            domain::render_generation_prompt(request, sources, self.config.limits.document_chars);
        info!(
            model = %self.config.llm.model,
            prompt_chars = prompt_text.chars().count(),
            sources = sources.len(),
            "sending generation request to OpenRouter"
        );

        llm::generate_with_retry(
            &agent,
            &prompt_text,
            &self.config.llm.model,
            self.config.max_generation_attempts(),
        )
        .await
    }
}

impl ExpansionRuntime for DocxExpansionService {
    async fn expand(
        &self,
        request: ExpansionRequest,
    ) -> Result<ExpansionResult, agent_core::BoxError> {
        let mut sources = self.collect_user_sources(&request.user_urls).await?;
        let mut search_queries = Vec::new();

        if let Some((query, results)) = self.collect_search_sources(&request).await? {
            search_queries.push(query);
            sources.extend(results);
        }

        info!(
            search_queries = search_queries.len(),
            sources = sources.len(),
            "collected supporting sources"
        );

        let content = self.generate_content(&request, &sources).await?;

        Ok(ExpansionResult {
            content,
            search_queries,
            sources,
        })
    }
}
