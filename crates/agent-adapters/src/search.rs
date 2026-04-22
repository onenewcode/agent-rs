use agent_kernel::{
    AgentError, ErrorType, OrErr, Result, RetryType, SearchProvider, SourceKind, SourceMaterial,
    truncate_chars,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize)]
struct TavilySearchResponse {
    results: Vec<TavilyResult>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TavilyResult {
    title: String,
    url: String,
    content: String,
}

pub struct TavilySearchProvider {
    api_key: String,
    base_url: String,
    client: Arc<reqwest::Client>,
}

impl TavilySearchProvider {
    #[must_use]
    pub fn new(api_key: String, client: Arc<reqwest::Client>, base_url: Option<String>) -> Self {
        Self {
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.tavily.com/search".to_owned()),
            client,
        }
    }

    async fn search_tavily(&self, query: &str, max_results: usize) -> Result<Vec<SourceMaterial>> {
        let request_body = json!({
            "api_key": &self.api_key,
            "query": query,
            "search_depth": "advanced",
            "max_results": max_results,
        });

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.client.post(&self.base_url).json(&request_body).send(),
        )
        .await
        .map_err(|_| {
            AgentError::explain(
                ErrorType::Timeout,
                format!("search timed out for query `{query}`"),
            )
        })?
        .or_err(ErrorType::Network, "Tavily search request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_owned());
            let mut err = AgentError::explain(
                ErrorType::Provider,
                format!("Tavily API error ({status}): {body}"),
            );
            if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                err = err.set_retry(RetryType::Retry);
            }
            return Err(err);
        }

        let search_response: TavilySearchResponse = response
            .json()
            .await
            .or_err(ErrorType::Provider, "failed to parse Tavily response")?;

        let materials = search_response
            .results
            .into_iter()
            .map(|r| SourceMaterial {
                title: Some(r.title),
                url: r.url,
                content: truncate_chars(&r.content, 4000),
                kind: SourceKind::SearchResult,
                summary: None,
            })
            .collect();

        Ok(materials)
    }
}

impl SearchProvider for TavilySearchProvider {
    fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> agent_kernel::BoxFuture<'_, Result<Vec<SourceMaterial>>> {
        let query = query.to_owned();
        Box::pin(async move { self.search_tavily(&query, max_results).await })
    }
}
