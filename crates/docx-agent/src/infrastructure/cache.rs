use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use agent_core::{BoxError, BoxFuture, ExpansionError, FetchedSource, UrlFetcher};
use sha2::{Digest, Sha256};
use tokio::fs;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct DiskCacheFetcher<F: UrlFetcher> {
    inner: F,
    cache_dir: PathBuf,
    max_age_days: u64,
}

impl<F: UrlFetcher> DiskCacheFetcher<F> {
    pub fn new(inner: F, cache_dir: impl Into<PathBuf>, max_age_days: u64) -> Self {
        let cache_dir = cache_dir.into();
        let fetcher = Self {
            inner,
            cache_dir: cache_dir.clone(),
            max_age_days,
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
    fn fetch(&self, url: &str) -> BoxFuture<'_, Result<FetchedSource, ExpansionError>> {
        let url = url.to_owned();
        let path = self.cache_path(&url);
        let cache_dir = self.cache_dir.clone();
        let max_age = Duration::from_secs(self.max_age_days * 24 * 3600);

        Box::pin(async move {
            if path.exists() {
                let is_valid = if let Ok(metadata) = fs::metadata(&path).await {
                    if let Ok(modified) = metadata.modified() {
                        if let Ok(age) = SystemTime::now().duration_since(modified) {
                            age < max_age
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                if is_valid
                    && let Ok(content) = fs::read_to_string(&path).await
                    && let Ok(source) = serde_json::from_str::<FetchedSource>(&content)
                {
                    info!(url, path = %path.display(), "cache hit for disk fetcher");
                    return Ok(source);
                } else if !is_valid {
                    info!(url, path = %path.display(), "cache expired or invalid, refetching");
                }
            }

            let source = self.inner.fetch(&url).await?;

            if let Ok(()) = fs::create_dir_all(&cache_dir).await
                && let Ok(content) = serde_json::to_string(&source)
                && let Err(e) = fs::write(&path, content).await
            {
                warn!(
                    url,
                    path = %path.display(),
                    error = %e,
                    "failed to write cache file"
                );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use agent_core::SourceKind;

    struct MockFetcher(Arc<AtomicUsize>);
    impl UrlFetcher for MockFetcher {
        fn fetch(&self, url: &str) -> BoxFuture<'_, Result<FetchedSource, ExpansionError>> {
            let url = url.to_owned();
            let count = self.0.clone();
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(FetchedSource {
                    kind: SourceKind::UserUrl,
                    title: Some("Mock".to_owned()),
                    url,
                    summary: None,
                    content: "Fresh Content".to_owned(),
                })
            })
        }
    }

    #[tokio::test]
    async fn test_disk_cache_enforces_expiration() -> Result<(), BoxError> {
        let temp_dir = std::env::temp_dir().join(format!("agent-cache-test-{}", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_millis()));
        let url = "https://example.com/stale";
        let call_count = Arc::new(AtomicUsize::new(0));
        let fetcher = DiskCacheFetcher::new(MockFetcher(call_count.clone()), &temp_dir, 1);

        // 1. Initial fetch (cache miss)
        let res1 = fetcher.fetch(url).await?;
        assert_eq!(res1.content, "Fresh Content");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // 2. Immediate fetch (cache hit)
        let res2 = fetcher.fetch(url).await?;
        assert_eq!(res2.content, "Fresh Content");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        // 3. Manually backdate the cache file to make it stale (2 days old)
        let cache_path = fetcher.cache_path(url);
        let stale_time = SystemTime::now() - Duration::from_hours(48);
        filetime::set_file_mtime(&cache_path, filetime::FileTime::from_system_time(stale_time))?;

        // 4. Fetch again (cache expired, should refetch)
        let res3 = fetcher.fetch(url).await?;
        assert_eq!(res3.content, "Fresh Content");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);

        // Cleanup
        fs::remove_dir_all(temp_dir).await?;
        Ok(())
    }
}
