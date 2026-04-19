use agent_core::{BoxFuture, FetchedSource, SearchBackend, SourceKind, truncate_chars};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::DocxAgentError;

const TAVILY_SEARCH_ENDPOINT: &str = "https://api.tavily.com/search";

#[derive(Debug, Clone)]
pub struct TavilySearchClient {
    client: reqwest::Client,
    api_key: String,
    max_chars: usize,
}

impl TavilySearchClient {
    #[must_use]
    pub fn new(client: reqwest::Client, api_key: &str, max_chars: usize) -> Self {
        Self {
            client,
            api_key: api_key.to_owned(),
            max_chars,
        }
    }

    pub async fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<FetchedSource>, DocxAgentError> {
        let query_chars = query.chars().count();
        info!(query_chars, max_results, "starting Tavily search");

        let response = self
            .client
            .post(TAVILY_SEARCH_ENDPOINT)
            .json(&TavilySearchRequest {
                api_key: self.api_key.clone(),
                query: query.to_owned(),
                topic: "general".to_owned(),
                search_depth: "basic".to_owned(),
                max_results,
                include_answer: false,
                include_images: false,
                include_raw_content: false,
            })
            .send()
            .await?;

        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            warn!(query_chars, status = %status, "Tavily search request failed");
            debug!(
                query_chars,
                status = %status,
                response_preview = %truncate_for_log(&body),
                "Tavily failure response preview"
            );
            return Err(DocxAgentError::Agent(agent_core::ExpansionError::Provider(
                format!("tavily search failed with status {status}"),
            )));
        }

        let payload: TavilySearchResponse = serde_json::from_str(&body).map_err(|error| {
            warn!(query_chars, "failed to parse Tavily response");
            debug!(
                query_chars,
                status = %status,
                response_preview = %truncate_for_log(&body),
                "Tavily parse failure response preview"
            );
            DocxAgentError::Agent(agent_core::ExpansionError::Provider(format!(
                "failed to parse Tavily response: {error}"
            )))
        })?;

        let results: Vec<FetchedSource> = payload
            .results
            .into_iter()
            .map(|result| to_fetched_source(result, self.max_chars))
            .collect();

        info!(
            query_chars,
            results = results.len(),
            "completed Tavily search"
        );
        Ok(results)
    }
}

impl SearchBackend for TavilySearchClient {
    fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> BoxFuture<'_, Result<Vec<FetchedSource>, agent_core::ExpansionError>> {
        let query = query.to_owned();
        Box::pin(async move {
            self.search(&query, max_results)
                .await
                .map_err(|e| agent_core::ExpansionError::Provider(e.to_string()))
        })
    }
}

fn none_if_empty(value: impl AsRef<str>) -> Option<String> {
    let trimmed = value.as_ref().trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn truncate_for_log(value: &str) -> String {
    const LIMIT: usize = 400;
    let truncated: String = value.chars().take(LIMIT).collect();
    if value.chars().count() > LIMIT {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn to_fetched_source(result: TavilySearchResult, max_chars: usize) -> FetchedSource {
    let content = truncate_chars(result.content.trim(), max_chars);
    FetchedSource {
        kind: SourceKind::SearchResult,
        title: Some(result.title),
        url: result.url,
        summary: none_if_empty(&content),
        content,
    }
}

#[derive(Debug, Serialize)]
struct TavilySearchRequest {
    api_key: String,
    query: String,
    topic: String,
    search_depth: String,
    max_results: usize,
    include_answer: bool,
    include_images: bool,
    include_raw_content: bool,
}

#[derive(Debug, Deserialize)]
struct TavilySearchResponse {
    #[serde(default)]
    results: Vec<TavilySearchResult>,
}

#[derive(Debug, Deserialize)]
struct TavilySearchResult {
    title: String,
    url: String,
    #[serde(default)]
    content: String,
}

#[cfg(test)]
mod tests {
    use super::{TavilySearchResult, to_fetched_source};
    use agent_core::SourceKind;

    #[test]
    fn tavily_result_content_respects_source_char_limit() {
        let result = TavilySearchResult {
            title: "title".to_owned(),
            url: "https://example.com".to_owned(),
            content: "abcdefgh".to_owned(),
        };

        let source = to_fetched_source(result, 5);
        assert_eq!(source.kind, SourceKind::SearchResult);
        assert_eq!(source.content, "abcde");
        assert_eq!(source.summary.as_deref(), Some("abcde"));
    }
}
