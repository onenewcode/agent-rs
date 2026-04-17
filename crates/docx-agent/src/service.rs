use std::{path::Path, time::Duration};

use agent_core::{
    truncate_chars, DocumentParser, ExpansionRequest, ExpansionResult, ExpansionRuntime,
    FetchedSource, ParsedDocument, SearchBackend, UrlFetcher,
};
use async_trait::async_trait;
use rig::{client::CompletionClient, completion::Prompt, providers::openrouter};
use tracing::{info, warn};

use crate::{
    config::DocxAgentConfig, error::DocxAgentError, fetch::WebPageFetcher,
    parser::DocxDocumentParser, search::TavilySearchClient,
};

pub struct DocxExpansionService {
    config: DocxAgentConfig,
    parser: Box<dyn DocumentParser>,
    url_fetcher: Box<dyn UrlFetcher>,
    search_backend: Option<Box<dyn SearchBackend>>,
}

impl DocxExpansionService {
    pub fn from_config_path(path: &Path) -> Result<Self, DocxAgentError> {
        let config = DocxAgentConfig::from_path(path)?;
        let search_backend = config
            .search
            .api_key
            .as_deref()
            .map(|api_key| {
                TavilySearchClient::new(
                    api_key,
                    &config.fetch.user_agent,
                    config.limits.source_chars,
                )
            })
            .transpose()?
            .map(|client| Box::new(client) as Box<dyn SearchBackend>);
        let url_fetcher = Box::new(WebPageFetcher::new(
            &config.fetch.user_agent,
            config.limits.source_chars,
        )?) as Box<dyn UrlFetcher>;

        info!(
            search_enabled = search_backend.is_some(),
            source_char_limit = config.limits.source_chars,
            document_char_limit = config.limits.document_chars,
            "initialized docx expansion service"
        );

        Ok(Self {
            config,
            parser: Box::new(DocxDocumentParser),
            url_fetcher,
            search_backend,
        })
    }

    pub fn parse_document(&self, path: &Path) -> Result<ParsedDocument, DocxAgentError> {
        self.parser.parse_path(path).map_err(into_docx_agent_error)
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
        .map_err(into_docx_agent_error)
    }

    async fn collect_supporting_sources(
        &self,
        request: &ExpansionRequest,
    ) -> Result<(Vec<String>, Vec<FetchedSource>), DocxAgentError> {
        let mut sources = self.fetch_user_urls(&request.user_urls).await?;

        let search_result = self.search_if_needed(request).await?;
        let mut search_queries = Vec::new();
        if let Some((query, results)) = search_result {
            search_queries.push(query);
            sources.extend(results);
        }

        info!(
            search_queries = search_queries.len(),
            sources = sources.len(),
            "collected supporting sources"
        );
        Ok((search_queries, sources))
    }

    async fn fetch_user_urls(
        &self,
        urls: &[String],
    ) -> Result<Vec<FetchedSource>, DocxAgentError> {
        let mut sources = Vec::new();
        for url in urls {
            sources.push(
                self.url_fetcher
                    .fetch(url)
                    .await
                    .map_err(into_docx_agent_error)?,
            );
        }
        Ok(sources)
    }

    async fn search_if_needed(
        &self,
        request: &ExpansionRequest,
    ) -> Result<Option<(String, Vec<FetchedSource>)>, DocxAgentError> {
        let search_requested = self.config.search_policy.should_search(&request.prompt);
        info!(
            search_requested,
            search_enabled = self.search_backend.is_some(),
            user_urls = request.user_urls.len(),
            "evaluated external research policy"
        );

        if !search_requested {
            return Ok(None);
        }

        let query = build_search_query(request);
        if let Some(backend) = self.search_backend.as_ref() {
            let results = backend
                .search(&query, self.config.search.max_results)
                .await
                .map_err(into_docx_agent_error)?;
            if results.is_empty() {
                return Ok(None);
            }
            Ok(Some((query, results)))
        } else {
            warn!("prompt requested search but no search API key is configured");
            Ok(None)
        }
    }

    fn build_openrouter_client(&self) -> Result<openrouter::Client, DocxAgentError> {
        let http_client = reqwest::Client::builder()
            .user_agent(&self.config.fetch.user_agent)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .build()?;
        openrouter::Client::builder()
            .api_key(self.config.llm.api_key.as_str())
            .http_client(http_client)
            .build()
            .map_err(|error| {
                DocxAgentError::Agent(format!("failed to build OpenRouter client: {error}"))
            })
    }

    fn build_agent(
        &self,
        client: &openrouter::Client,
    ) -> impl Prompt {
        client
            .agent(&self.config.llm.model)
            .preamble(self.config.system_prompt())
            .build()
    }

    async fn generate_content(
        &self,
        request: &ExpansionRequest,
        sources: &[FetchedSource],
    ) -> Result<String, DocxAgentError> {
        let client = self.build_openrouter_client()?;
        let agent = self.build_agent(&client);
        let prompt =
            render_generation_prompt(request, sources, self.config.limits.document_chars);
        info!(
            model = %self.config.llm.model,
            prompt_chars = prompt.chars().count(),
            sources = sources.len(),
            "sending generation request to OpenRouter"
        );

        generate_with_retry(
            &agent,
            &prompt,
            &self.config.llm.model,
            self.config.max_generation_attempts(),
        )
        .await
    }
}

#[async_trait]
impl ExpansionRuntime for DocxExpansionService {
    async fn expand(
        &self,
        request: ExpansionRequest,
    ) -> Result<ExpansionResult, agent_core::BoxError> {
        let (search_queries, sources) = self.collect_supporting_sources(&request).await?;
        let content = self.generate_content(&request, &sources).await?;

        Ok(ExpansionResult {
            content,
            search_queries,
            sources,
        })
    }
}

fn build_search_query(request: &ExpansionRequest) -> String {
    let mut parts = Vec::new();
    if let Some(title) = &request.document.title {
        parts.push(title.clone());
    }

    let trimmed_prompt = request.prompt.trim();
    if !trimmed_prompt.is_empty() {
        parts.push(trimmed_prompt.to_owned());
    }

    parts.join(" ")
}

fn render_generation_prompt(
    request: &ExpansionRequest,
    sources: &[FetchedSource],
    max_document_chars: usize,
) -> String {
    let document_markdown =
        truncate_chars(&request.document.render_markdown(), max_document_chars);
    let source_sections = if sources.is_empty() {
        "无".to_owned()
    } else {
        sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                format!(
                    "来源 {index}\n标题: {}\nURL: {}\n摘要: {}\n内容摘录:\n{}",
                    source.title.as_deref().unwrap_or("未提供"),
                    source.url,
                    source.summary.as_deref().unwrap_or("无"),
                    source.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    format!(
        "任务:\n{}\n\n文档:\n{}\n\n用户 URL:\n{}\n\n外部材料:\n{}\n\n请直接输出最终中文 Markdown。",
        request.prompt,
        document_markdown,
        if request.user_urls.is_empty() {
            "无".to_owned()
        } else {
            request.user_urls.join("\n")
        },
        source_sections
    )
}

async fn generate_with_retry(
    agent: &impl Prompt,
    prompt: &str,
    model: &str,
    max_attempts: usize,
) -> Result<String, DocxAgentError> {
    for attempt in 1..=max_attempts {
        match agent.prompt(prompt).await {
            Ok(content) => {
                info!(
                    model,
                    attempt,
                    output_chars = content.chars().count(),
                    "received generation response from OpenRouter"
                );
                return Ok(content);
            }
            Err(error) => {
                let message = error.to_string();
                let retryable = is_retryable_llm_error(&message);
                if retryable && attempt < max_attempts {
                    let delay = Duration::from_secs((attempt as u64) * 2);
                    warn!(
                        model,
                        attempt,
                        delay_secs = delay.as_secs(),
                        error = %message,
                        "OpenRouter request failed with a retryable error"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }

                return Err(DocxAgentError::Agent(message));
            }
        }
    }

    Err(DocxAgentError::Agent(
        "OpenRouter generation exhausted retries".to_owned(),
    ))
}

fn is_retryable_llm_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "429",
        "rate limit",
        "rate-limited",
        "rate limited",
        "too many requests",
        "temporarily rate-limited",
        "timeout",
        "timed out",
        "temporarily unavailable",
        "service unavailable",
        "connection reset",
        "deadline exceeded",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn into_docx_agent_error(error: agent_core::BoxError) -> DocxAgentError {
    match error.downcast::<DocxAgentError>() {
        Ok(error) => *error,
        Err(error) => DocxAgentError::Agent(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_search_query, is_retryable_llm_error};
    use crate::config::SearchPolicyConfig;
    use agent_core::{ExpansionRequest, ParsedDocument};

    #[test]
    fn search_policy_uses_prompt_hints() {
        let policy = SearchPolicyConfig::default();
        assert!(policy.should_search("请联网搜索行业最新案例并扩写"));
        assert!(!policy.should_search("请基于文档扩写，不要联网搜索"));
        assert!(!policy.should_search(
            "Please refine this draft, do not search the web."
        ));
        assert!(!policy.should_search("只做语气润色，不要补充事实"));
        assert!(policy.should_search(
            "Please search latest market data and then expand."
        ));
    }

    #[test]
    fn search_policy_is_case_insensitive() {
        let policy = SearchPolicyConfig::default();
        assert!(policy.should_search("LATEST data please"));
        assert!(policy.should_search("SEARCH for more info"));
        assert!(!policy.should_search("DO NOT SEARCH the web"));
        assert!(!policy.should_search("NO SEARCH needed"));
    }

    #[test]
    fn search_query_prefers_document_title_and_prompt() {
        let request = ExpansionRequest {
            prompt: "补充市场数据".to_owned(),
            document: ParsedDocument {
                title: Some("智能写作方案".to_owned()),
                blocks: vec![],
            },
            user_urls: vec![],
        };

        assert_eq!(build_search_query(&request), "智能写作方案 补充市场数据");
    }

    #[test]
    fn retryable_error_detection_covers_common_variants() {
        assert!(is_retryable_llm_error("HTTP 429 Too Many Requests"));
        assert!(is_retryable_llm_error("Provider is Rate Limited upstream"));
        assert!(is_retryable_llm_error("request timed out"));
        assert!(!is_retryable_llm_error("invalid api key"));
    }
}
