use agent_kernel::{
    Error, ErrorSource, ErrorType, OrErr, Result, RetryType, SourceFetcher, SourceKind,
    SourceMaterial, truncate_chars,
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
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.client.get(url).send(),
        )
        .await
        .map_err(|_| {
            Box::new(Error::explain(
                ErrorType::Timeout,
                format!("fetching {url} timed out"),
            ))
        })?
        .or_err(ErrorType::Network, &format!("failed to fetch {url}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let mut err = Error::explain(
                ErrorType::Network,
                format!("failed to fetch {url}: HTTP {status}"),
            );
            if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                err = err.set_retry(RetryType::Retry);
            }
            return Err(Box::new(err.set_source(ErrorSource::Upstream)));
        }

        let content = response.text().await.or_err(
            ErrorType::Network,
            &format!("failed to read response text from {url}"),
        )?;

        Ok(SourceMaterial {
            title: Some(url.to_owned()),
            url: url.to_owned(),
            content: truncate_chars(&content, 10000),
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
