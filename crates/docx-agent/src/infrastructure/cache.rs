use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use agent_core::{BoxError, BoxFuture, FetchedSource, UrlFetcher};
use sha2::{Digest, Sha256};
use tokio::fs;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct DiskCacheFetcher<F: UrlFetcher> {
    inner: F,
    cache_dir: PathBuf,
}

impl<F: UrlFetcher> DiskCacheFetcher<F> {
    pub fn new(inner: F, cache_dir: impl Into<PathBuf>, max_age_days: u64) -> Self {
        let cache_dir = cache_dir.into();
        let fetcher = Self {
            inner,
            cache_dir: cache_dir.clone(),
        };

        // Run pruning in the background or just spawn a task
        tokio::spawn(async move {
            if let Err(e) = prune_stale_cache(&cache_dir, max_age_days).await {
                warn!(error = %e, "failed to prune stale cache");
            }
        });

        fetcher
    }

    fn cache_path(&self, url: &str) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let hash = hex::encode(hasher.finalize());
        self.cache_dir.join(format!("{hash}.json"))
    }
}

impl<F: UrlFetcher> UrlFetcher for DiskCacheFetcher<F> {
    fn fetch(&self, url: &str) -> BoxFuture<'_, Result<FetchedSource, BoxError>> {
        let url = url.to_owned();
        let path = self.cache_path(&url);
        let cache_dir = self.cache_dir.clone();

        Box::pin(async move {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path).await {
                    if let Ok(source) = serde_json::from_str::<FetchedSource>(&content) {
                        info!(url, path = %path.display(), "cache hit for disk fetcher");
                        return Ok(source);
                    }
                }
            }

            let source = self.inner.fetch(&url).await?;

            if let Ok(()) = fs::create_dir_all(&cache_dir).await {
                if let Ok(content) = serde_json::to_string(&source) {
                    if let Err(e) = fs::write(&path, content).await {
                        warn!(
                            url,
                            path = %path.display(),
                            error = %e,
                            "failed to write cache file"
                        );
                    }
                }
            }

            Ok(source)
        })
    }
}

async fn prune_stale_cache(cache_dir: &Path, max_age_days: u64) -> Result<(), BoxError> {
    if !cache_dir.exists() {
        return Ok(());
    }

    let mut dir = fs::read_dir(cache_dir).await?;
    let max_age = Duration::from_secs(max_age_days * 24 * 3600);
    let now = SystemTime::now();

    while let Some(entry) = dir.next_entry().await? {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
            let metadata = fs::metadata(&path).await?;
            let modified = metadata.modified()?;
            if now.duration_since(modified).unwrap_or_default() > max_age {
                info!(path = %path.display(), "pruning stale cache file");
                fs::remove_file(path).await?;
            }
        }
    }

    Ok(())
}
