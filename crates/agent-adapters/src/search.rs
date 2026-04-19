use std::time::Duration;

use agent_kernel::{RunError, SearchProvider, SourceKind, SourceMaterial, truncate_chars};
use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use tracing::{debug, info, warn};

#[derive(Debug, Clone)]
pub struct TavilySearchProvider {
    client: reqwest::Client,
    api_key: String,
    max_chars: usize,
    timeout_secs: u64,
}

impl TavilySearchProvider {
    #[must_use]
    pub fn new(
        client: reqwest::Client,
        api_key: &str,
        max_chars: usize,
        timeout_secs: u64,
    ) -> Self {
        Self {
            client,
            api_key: api_key.to_owned(),
            max_chars,
            timeout_secs,
        }
    }

    async fn search_internal(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SourceMaterial>, RunError> {
        const ENDPOINT: &str = "https://api.tavily.com/search";
        let response = timeout(Duration::from_secs(self.timeout_secs), async {
            self.client
                .post(ENDPOINT)
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
                .await
        })
        .await
        .map_err(|_| RunError::Timeout(format!("search timed out for query `{query}`")))?
        .map_err(|error| RunError::Network(error.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| RunError::Network(error.to_string()))?;

        if !status.is_success() {
            warn!(query, status = %status, "Tavily search request failed");
            debug!(response_preview = %truncate_chars(&body, 400), "Tavily failure response preview");
            return Err(RunError::Provider(format!(
                "tavily search failed with status {status}"
            )));
        }

        let payload: TavilySearchResponse = serde_json::from_str(&body).map_err(|error| {
            RunError::Provider(format!("failed to parse Tavily response: {error}"))
        })?;
        let results = payload
            .results
            .into_iter()
            .map(|result| SourceMaterial {
                kind: SourceKind::SearchResult,
                title: Some(result.title),
                url: result.url,
                summary: none_if_empty(result.content.trim()),
                content: truncate_chars(result.content.trim(), self.max_chars),
            })
            .collect::<Vec<_>>();

        info!(query, results = results.len(), "completed Tavily search");
        Ok(results)
    }
}

impl SearchProvider for TavilySearchProvider {
    fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> agent_kernel::BoxFuture<'_, Result<Vec<SourceMaterial>, RunError>> {
        let query = query.to_owned();
        Box::pin(async move { self.search_internal(&query, max_results).await })
    }
}

fn none_if_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
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
    use super::none_if_empty;

    #[test]
    fn empty_strings_become_none() {
        assert_eq!(none_if_empty(""), None);
        assert_eq!(none_if_empty("  "), None);
        assert_eq!(none_if_empty("value").as_deref(), Some("value"));
    }
}
