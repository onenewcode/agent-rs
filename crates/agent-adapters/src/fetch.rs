use agent_kernel::{
    Error, ErrorType, Result, SourceFetcher, SourceKind, SourceMaterial, truncate_chars,
};
use std::sync::Arc;

pub struct ReqwestFetcher {
    client: Arc<reqwest::Client>,
}

impl ReqwestFetcher {
    #[must_use]
    pub fn new(client: Arc<reqwest::Client>) -> Self {
        Self { client }
    }

    async fn fetch_url(&self, url: &str) -> Result<SourceMaterial> {
        let jina_url = format!("https://r.jina.ai/{url}");

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(45), // Increased timeout for Jina
            self.client.get(&jina_url).send(),
        )
        .await
        .map_err(|_| {
            Box::new(Error::explain(
                ErrorType::Timeout,
                format!("fetching {url} via Jina timed out"),
            ))
        })?;

        // If the request itself failed (e.g. DNS), we still return Err
        let response = match response {
            Ok(resp) => resp,
            Err(e) => {
                return Err(Box::new(Error::because(
                    e,
                    ErrorType::Network,
                    format!("failed to send request to Jina for {url}"),
                )));
            }
        };

        // If the status is not success (e.g. 404), we return a GENTLE error message in the content
        // instead of a Rust Err. This allows the LLM to recover from its own hallucinations.
        if !response.status().is_success() {
            let status = response.status();
            return Ok(SourceMaterial {
                title: Some(format!("Error: {status}")),
                url: url.to_owned(),
                content: format!(
                    "FAILED to access the URL {url}. HTTP Status: {status}. \
                    This URL might be invalid or the site might be blocking scrapers. \
                    Please use another URL or rely on your search summaries."
                ),
                kind: SourceKind::UserUrl,
                summary: None,
            });
        }

        let content = match response.text().await {
            Ok(text) => text,
            Err(e) => {
                return Err(Box::new(Error::because(
                    e,
                    ErrorType::Network,
                    format!("failed to read Jina response body for {url}"),
                )));
            }
        };

        Ok(SourceMaterial {
            title: Some(url.to_owned()),
            url: url.to_owned(),
            content: truncate_chars(&content, 15000), // Clean Markdown from Jina
            kind: SourceKind::UserUrl,
            summary: None,
        })
    }
}

impl SourceFetcher for ReqwestFetcher {
    fn fetch(&self, url: &str) -> agent_kernel::BoxFuture<'_, Result<SourceMaterial>> {
        let url = url.to_owned();
        Box::pin(async move { self.fetch_url(&url).await })
    }
}
