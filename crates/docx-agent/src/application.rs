use std::{path::Path, sync::Arc, time::Duration};

use agent_core::{
    BoxFuture, ExpansionRequest, ExpansionResult, ExpansionRuntime, FetchedSource, SearchBackend,
    UrlFetcher,
};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::{info, warn};

use crate::{
    config::DocxAgentConfig,
    domain,
    error::DocxAgentError,
    infrastructure::{
        cache::DiskCacheFetcher, docx::DocxDocumentParser, fetch::WebPageFetcher, llm,
        search::TavilySearchClient,
    },
};

pub struct DocxExpansionService {
    config: DocxAgentConfig,
    http: reqwest::Client,
    search_client: Option<Arc<dyn SearchBackend>>,
    fetcher: Arc<dyn UrlFetcher>,
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

        let search_client: Option<Arc<dyn SearchBackend>> =
            config.search.api_key.as_deref().map(|api_key| {
                Arc::new(TavilySearchClient::new(
                    http.clone(),
                    api_key,
                    config.limits.source_tokens * 4,
                )) as Arc<dyn SearchBackend>
            });

        let base_fetcher = WebPageFetcher::new(http.clone(), config.limits.source_tokens * 4);
        let fetcher: Arc<dyn UrlFetcher> = Arc::new(DiskCacheFetcher::new(
            base_fetcher,
            &config.fetch.cache_dir,
            config.fetch.max_cache_age_days,
        ));

        info!(
            search_enabled = search_client.is_some(),
            source_token_limit = config.limits.source_tokens,
            document_token_limit = config.limits.document_tokens,
            cache_dir = %config.fetch.cache_dir,
            "initialized docx expansion service with persistent caching"
        );

        Ok(Self {
            config,
            http,
            search_client,
            fetcher,
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

        let semaphore = Arc::new(Semaphore::new(self.config.fetch.concurrency_limit));
        let mut set = tokio::task::JoinSet::new();
        let timeout_dur = Duration::from_secs(self.config.fetch.timeout_secs);

        for url in urls {
            let f = Arc::clone(&self.fetcher);
            let u = url.clone();
            let sem = Arc::clone(&semaphore);
            set.spawn(async move {
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|e| DocxAgentError::Agent(Box::new(e)))?;
                match timeout(timeout_dur, f.fetch(&u)).await {
                    Ok(res) => res.map_err(DocxAgentError::Agent),
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

        let agent = llm::build_agent(&self.http, &self.config)?;
        let query = match llm::generate_optimized_search_query(
            &agent,
            request.document.title.as_deref(),
            &request.prompt,
            &self.config.llm.model,
        )
        .await
        {
            Ok(q) => q,
            Err(e) => {
                warn!(error = %e, "LLM search query generation failed, using fallback");
                domain::build_fallback_search_query(request)
            }
        };

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

        // Phase 1: Generate Outline
        let outline_prompt = domain::render_outline_prompt(
            self.config.outline_template(),
            request,
            sources,
            self.config.limits.document_tokens,
            self.config.limits.source_tokens,
        );

        info!(model = %self.config.llm.model, "generating expansion outline");
        let outline = llm::generate_with_retry(
            &agent,
            &outline_prompt,
            &self.config.llm.model,
            self.config.max_generation_attempts(),
        )
        .await?;

        // Phase 2: Generate Final Content
        let prompt_text = domain::render_generation_prompt(
            self.config.generation_template(),
            request,
            sources,
            &outline,
            self.config.limits.document_tokens,
            self.config.limits.source_tokens,
        );

        info!(
            model = %self.config.llm.model,
            prompt_tokens = prompt_text.len() / 4, // Rough estimate for logging
            sources = sources.len(),
            "sending final generation request to OpenRouter"
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
    fn expand(
        &self,
        request: ExpansionRequest,
    ) -> BoxFuture<'_, Result<ExpansionResult, agent_core::BoxError>> {
        let global_timeout = Duration::from_secs(self.config.limits.global_timeout_secs);

        Box::pin(async move {
            timeout(global_timeout, async {
                let (user_sources_res, search_sources_res) = tokio::join!(
                    self.collect_user_sources(&request.user_urls),
                    self.collect_search_sources(&request)
                );

                let mut sources = user_sources_res?;
                let mut search_queries = Vec::new();

                if let Some((query, results)) = search_sources_res? {
                    search_queries.push(query);
                    sources.extend(results);
                }

                info!(
                    search_queries = search_queries.len(),
                    sources = sources.len(),
                    "collected supporting sources"
                );

                let content = self.generate_content(&request, &sources).await?;

                Ok::<_, DocxAgentError>(ExpansionResult {
                    content,
                    search_queries,
                    sources,
                })
            })
            .await
            .map_err(|_| DocxAgentError::ResearchError {
                kind: "global_timeout",
                message: format!(
                    "total expansion process timed out after {}s",
                    global_timeout.as_secs()
                ),
            })?
            .map_err(Into::into)
        })
    }
}
