use std::collections::HashMap;
use std::sync::Arc;

use agent_core::{BoxError, BoxFuture, FetchedSource, UrlFetcher};
use tokio::sync::RwLock;
use tracing::info;

#[derive(Debug, Clone)]
pub struct InMemoryCacheFetcher<F: UrlFetcher> {
    inner: F,
    cache: Arc<RwLock<HashMap<String, FetchedSource>>>,
}

impl<F: UrlFetcher> InMemoryCacheFetcher<F> {
    pub fn new(inner: F) -> Self {
        Self {
            inner,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl<F: UrlFetcher> UrlFetcher for InMemoryCacheFetcher<F> {
    fn fetch(&self, url: &str) -> BoxFuture<'_, Result<FetchedSource, BoxError>> {
        let url = url.to_owned();
        Box::pin(async move {
            {
                let cache = self.cache.read().await;
                if let Some(source) = cache.get(&url) {
                    info!(url, "cache hit for URL fetcher");
                    return Ok(source.clone());
                }
            }

            let source = self.inner.fetch(&url).await?;

            {
                let mut cache = self.cache.write().await;
                cache.insert(url, source.clone());
            }

            Ok(source)
        })
    }
}
