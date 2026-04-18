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
    search_client: Option<Arc<dyn SearchBackend>>,
    fetcher: Arc<dyn UrlFetcher>,
    llm_client: Option<Arc<dyn agent_core::LlmBackend>>,
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

        let llm_client: Arc<dyn agent_core::LlmBackend> =
            Arc::new(llm::build_agent(http, config.clone())?);

        info!(
            search_enabled = search_client.is_some(),
            source_token_limit = config.limits.source_tokens,
            document_token_limit = config.limits.document_tokens,
            cache_dir = %config.fetch.cache_dir,
            "initialized docx expansion service with persistent caching"
        );

        Ok(Self {
            config,
            search_client,
            fetcher,
            llm_client: Some(llm_client),
        })
    }

    #[must_use]
    pub fn new_with_infra(
        config: DocxAgentConfig,
        search_client: Option<Arc<dyn SearchBackend>>,
        fetcher: Arc<dyn UrlFetcher>,
        llm_client: Option<Arc<dyn agent_core::LlmBackend>>,
    ) -> Self {
        Self {
            config,
            search_client,
            fetcher,
            llm_client,
        }
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
                    .map_err(|e| agent_core::ExpansionError::Internal(e.to_string()))?;
                match timeout(timeout_dur, f.fetch(&u)).await {
                    Ok(res) => res,
                    Err(_) => Err(agent_core::ExpansionError::Timeout(format!(
                        "fetching {u} timed out after {}s",
                        timeout_dur.as_secs()
                    ))),
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

        let Some(backend) = self.search_client.as_ref() else {
            tracing::warn!("prompt requested search but no search API key is configured");
            return Ok(None);
        };

        let llm = self.llm_client.as_ref().ok_or_else(|| {
            DocxAgentError::Agent(agent_core::ExpansionError::Internal(
                "LLM client not initialized".to_owned(),
            ))
        })?;

        let query = match llm::generate_optimized_search_query(
            &**llm,
            request.document.title.as_deref(),
            &request.prompt,
            &self.config,
        )
        .await
        {
            Ok(q) => q,
            Err(e) => {
                warn!(error = %e, "LLM search query generation failed, using fallback");
                domain::build_fallback_search_query(request)
            }
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
        let budgeter = domain::ContextBudgeter::new(self.config.limits.max_total_tokens());

        let agent = self.llm_client.as_ref().ok_or_else(|| {
            DocxAgentError::Agent(agent_core::ExpansionError::Internal(
                "LLM client not initialized".to_owned(),
            ))
        })?;

        // Phase 1: Generate Outline
        let outline_prompt = domain::render_outline_prompt(
            self.config.outline_template(),
            request,
            sources,
            &budgeter,
        );

        info!(model = %self.config.llm.model, "generating expansion outline");
        let outline = agent.prompt(&outline_prompt).await.map_err(DocxAgentError::Agent)?;

        // Phase 2: Generate Final Content
        let prompt_text = domain::render_generation_prompt(
            self.config.generation_template(),
            request,
            sources,
            &outline,
            &budgeter,
        );

        info!(
            model = %self.config.llm.model,
            prompt_tokens = domain::count_tokens(&prompt_text),
            sources = sources.len(),
            "sending final generation request"
        );

        agent.prompt(&prompt_text).await.map_err(DocxAgentError::Agent)
    }
}

impl ExpansionRuntime for DocxExpansionService {
    fn expand(
        &self,
        request: ExpansionRequest,
    ) -> BoxFuture<'_, Result<ExpansionResult, agent_core::ExpansionError>> {
        let global_timeout = Duration::from_secs(self.config.limits.global_timeout_secs);

        Box::pin(async move {
            timeout(global_timeout, async {
                let (user_sources_res, search_sources_res) = tokio::join!(
                    self.collect_user_sources(&request.user_urls),
                    self.collect_search_sources(&request)
                );

                let mut sources = user_sources_res.map_err(|e| {
                    agent_core::ExpansionError::Internal(format!("User sources collection failed: {e}"))
                })?;
                let mut search_queries = Vec::new();

                if let Some((query, results)) = search_sources_res.map_err(|e| {
                    agent_core::ExpansionError::Internal(format!("Search sources collection failed: {e}"))
                })? {
                    search_queries.push(query);
                    sources.extend(results);
                }

                info!(
                    search_queries = search_queries.len(),
                    sources = sources.len(),
                    "collected supporting sources"
                );

                let content = self.generate_content(&request, &sources).await.map_err(|e| {
                    match e {
                        DocxAgentError::Agent(inner) => inner,
                        _ => agent_core::ExpansionError::Internal(e.to_string()),
                    }
                })?;

                Ok::<_, agent_core::ExpansionError>(ExpansionResult {
                    content,
                    search_queries,
                    sources,
                })
            })
            .await
            .map_err(|_| {
                agent_core::ExpansionError::Timeout(format!(
                    "total expansion process timed out after {}s",
                    global_timeout.as_secs()
                ))
            })?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{BlockKind, DocumentBlock, ParsedDocument, SourceKind};

    struct MockUrlFetcher;
    impl UrlFetcher for MockUrlFetcher {
        fn fetch(&self, url: &str) -> BoxFuture<'_, Result<FetchedSource, agent_core::ExpansionError>> {
            let url = url.to_owned();
            Box::pin(async move {
                Ok(FetchedSource {
                    kind: SourceKind::UserUrl,
                    title: Some("Mock Title".to_owned()),
                    url,
                    summary: None,
                    content: "Mock Web Content".to_owned(),
                })
            })
        }
    }

    struct MockSearchBackend;
    impl SearchBackend for MockSearchBackend {
        fn search(
            &self,
            _query: &str,
            _max_results: usize,
        ) -> BoxFuture<'_, Result<Vec<FetchedSource>, agent_core::ExpansionError>> {
            Box::pin(async move {
                Ok(vec![FetchedSource {
                    kind: SourceKind::SearchResult,
                    title: Some("Search Mock".to_owned()),
                    url: "https://search.com".to_owned(),
                    summary: None,
                    content: "Search Result Content".to_owned(),
                }])
            })
        }
    }

    struct MockLlm {
        responses: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl agent_core::LlmBackend for MockLlm {
        fn prompt(
            &self,
            _prompt: &str,
        ) -> agent_core::BoxFuture<'_, Result<String, agent_core::ExpansionError>> {
            let res = self.responses.lock().unwrap().remove(0);
            Box::pin(async move { Ok(res) })
        }
    }

    #[tokio::test]
    async fn test_expansion_pipeline_orchestration() -> Result<(), agent_core::ExpansionError> {
        let config_toml = r#"
            [llm]
            provider = "openrouter"
            model = "openai/gpt-4o-mini"
            api_key = "test-key"

            [search]
            provider = "tavily"
            api_key = "test-search-key"
            max_results = 2

            [limits]
            document_tokens = 100
            source_tokens = 100
            max_total_tokens = 1000

            [fetch]
            user_agent = "test"
        "#;
        let config: DocxAgentConfig = toml::from_str(config_toml).unwrap();
        let mock_llm = Arc::new(MockLlm {
            responses: Arc::new(std::sync::Mutex::new(vec![
                "Mock Outline".to_owned(),
                "Mock Final Content".to_owned(),
            ])),
        });

        let service = DocxExpansionService::new_with_infra(
            config,
            Some(Arc::new(MockSearchBackend)),
            Arc::new(MockUrlFetcher),
            Some(mock_llm),
        );

        let request = ExpansionRequest {
            prompt: "扩写这个文档，不要联网".to_owned(),
            document: ParsedDocument {
                title: Some("Test Doc".to_owned()),
                blocks: vec![DocumentBlock {
                    kind: BlockKind::Paragraph,
                    text: "Original text".to_owned(),
                }],
            },
            user_urls: vec!["https://example.com".to_owned()],
        };

        let result = service.expand(request.clone()).await?;
        assert_eq!(result.content, "Mock Final Content");
        assert_eq!(result.sources.len(), 1); // Only user URL, no search

        let user_sources = service.collect_user_sources(&request.user_urls).await
            .map_err(|e| agent_core::ExpansionError::Internal(e.to_string()))?;
        assert_eq!(user_sources.len(), 1);
        assert_eq!(user_sources[0].url, "https://example.com");

        let search_sources = service.collect_search_sources(&request).await
            .map_err(|e| agent_core::ExpansionError::Internal(e.to_string()))?;
        // "不要联网" should trigger the negative policy
        assert!(search_sources.is_none());

        Ok(())
    }
}
