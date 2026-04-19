use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use agent_kernel::{
    RunError, SourceFetcher, SourceKind, SourceMaterial, normalize_whitespace, truncate_chars,
};
use reqwest::header::CONTENT_TYPE;
use scraper::{ElementRef, Html, Selector};
use sha2::{Digest, Sha256};
use tokio::{fs, time::timeout};
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct WebPageSourceFetcher {
    client: reqwest::Client,
    max_chars: usize,
    timeout_secs: u64,
}

impl WebPageSourceFetcher {
    #[must_use]
    pub fn new(client: reqwest::Client, max_chars: usize, timeout_secs: u64) -> Self {
        Self {
            client,
            max_chars,
            timeout_secs,
        }
    }

    async fn fetch_url(&self, url: &str) -> Result<SourceMaterial, RunError> {
        let response = timeout(Duration::from_secs(self.timeout_secs), async {
            self.client.get(url).send().await
        })
        .await
        .map_err(|_| RunError::Timeout(format!("fetching {url} timed out")))?
        .map_err(|error| RunError::Network(error.to_string()))?
        .error_for_status()
        .map_err(|error| RunError::Network(error.to_string()))?;

        if let Some(content_type) = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|header| header.to_str().ok())
            && !is_supported_html_content_type(content_type)
        {
            return Err(RunError::Network(format!(
                "unsupported URL content type: {content_type}"
            )));
        }

        let body = response
            .text()
            .await
            .map_err(|error| RunError::Network(error.to_string()))?;
        let document = Html::parse_document(&body);
        let title = select_first_text(&document, "title");
        let content = extract_body_text(&document)
            .map(|text| truncate_chars(&text, self.max_chars))
            .unwrap_or_default();

        Ok(SourceMaterial {
            kind: SourceKind::UserUrl,
            title,
            url: url.to_owned(),
            summary: None,
            content,
        })
    }
}

impl SourceFetcher for WebPageSourceFetcher {
    fn fetch(&self, url: &str) -> agent_kernel::BoxFuture<'_, Result<SourceMaterial, RunError>> {
        let url = url.to_owned();
        Box::pin(async move { self.fetch_url(&url).await })
    }
}

#[derive(Debug, Clone)]
pub struct DiskCacheSourceFetcher<F: SourceFetcher> {
    inner: F,
    cache_dir: PathBuf,
    max_age_days: u64,
}

impl<F: SourceFetcher> DiskCacheSourceFetcher<F> {
    pub fn new(inner: F, cache_dir: impl Into<PathBuf>, max_age_days: u64) -> Self {
        let cache_dir = cache_dir.into();
        let fetcher = Self {
            inner,
            cache_dir: cache_dir.clone(),
            max_age_days,
        };

        tokio::spawn(async move {
            if let Err(error) = prune_stale_cache(&cache_dir, max_age_days).await {
                warn!(error = %error, "failed to prune stale cache");
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

impl<F: SourceFetcher> SourceFetcher for DiskCacheSourceFetcher<F> {
    fn fetch(&self, url: &str) -> agent_kernel::BoxFuture<'_, Result<SourceMaterial, RunError>> {
        let url = url.to_owned();
        let path = self.cache_path(&url);
        let cache_dir = self.cache_dir.clone();
        let max_age = Duration::from_secs(self.max_age_days * 24 * 3600);

        Box::pin(async move {
            if path.exists() {
                let is_valid = is_cache_valid(&path, max_age).await;
                if is_valid
                    && let Ok(content) = fs::read_to_string(&path).await
                    && let Ok(source) = serde_json::from_str::<SourceMaterial>(&content)
                {
                    info!(url, path = %path.display(), "cache hit for source fetcher");
                    return Ok(source);
                }
            }

            let source = self.inner.fetch(&url).await?;

            if let Ok(()) = fs::create_dir_all(&cache_dir).await
                && let Ok(content) = serde_json::to_string(&source)
                && let Err(error) = fs::write(&path, content).await
            {
                warn!(path = %path.display(), error = %error, "failed to write cache file");
            }

            Ok(source)
        })
    }
}

async fn is_cache_valid(path: &Path, max_age: Duration) -> bool {
    let Ok(metadata) = fs::metadata(path).await else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    let Ok(age) = SystemTime::now().duration_since(modified) else {
        return false;
    };
    age < max_age
}

async fn prune_stale_cache(cache_dir: &Path, max_age_days: u64) -> Result<(), RunError> {
    if !cache_dir.exists() {
        return Ok(());
    }

    let mut dir = fs::read_dir(cache_dir)
        .await
        .map_err(|error| RunError::Internal(error.to_string()))?;
    let max_age = Duration::from_secs(max_age_days * 24 * 3600);
    let now = SystemTime::now();

    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|error| RunError::Internal(error.to_string()))?
    {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|value| value.to_str()) == Some("json") {
            let metadata = fs::metadata(&path)
                .await
                .map_err(|error| RunError::Internal(error.to_string()))?;
            let modified = metadata
                .modified()
                .map_err(|error| RunError::Internal(error.to_string()))?;
            if now.duration_since(modified).unwrap_or_default() > max_age {
                fs::remove_file(path)
                    .await
                    .map_err(|error| RunError::Internal(error.to_string()))?;
            }
        }
    }

    Ok(())
}

fn select_first_text(document: &Html, selector: &str) -> Option<String> {
    Selector::parse(selector).ok().and_then(|selector| {
        document
            .select(&selector)
            .map(|node| normalize_whitespace(&node.text().collect::<String>()))
            .find(|text| !text.is_empty())
    })
}

fn extract_body_text(document: &Html) -> Option<String> {
    let selector = Selector::parse("body").ok()?;
    let body = document.select(&selector).next()?;

    let mut blocks = Vec::new();
    find_content_blocks(body, &mut blocks);
    let high_density_blocks: Vec<String> = blocks
        .into_iter()
        .filter(|block| {
            let total_len = block.text.len();
            let link_len = block.link_text_len;
            #[allow(clippy::cast_precision_loss)]
            let density = total_len as f32 / (link_len as f32 + 1.0);
            total_len > 40 && density > 2.0
        })
        .map(|block| block.text)
        .collect();

    if high_density_blocks.is_empty() {
        let mut raw = String::new();
        collect_raw_text(body, &mut raw);
        let normalized = normalize_whitespace(&raw);
        return if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        };
    }

    Some(normalize_whitespace(&high_density_blocks.join(" ")))
}

struct TextBlock {
    text: String,
    link_text_len: usize,
}

fn find_content_blocks(element: ElementRef<'_>, out: &mut Vec<TextBlock>) {
    if is_non_content_element(element.value().name()) {
        return;
    }

    if is_block_element(element.value().name()) {
        let mut text = String::new();
        let mut link_len = 0;
        collect_block_stats(element, &mut text, &mut link_len);
        let trimmed = text.trim().to_owned();
        if !trimmed.is_empty() {
            out.push(TextBlock {
                text: trimmed,
                link_text_len: link_len,
            });
        }
    }

    for child in element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            find_content_blocks(child_element, out);
        }
    }
}

fn collect_block_stats(element: ElementRef<'_>, text_out: &mut String, link_len_out: &mut usize) {
    let tag_name = element.value().name();
    let is_link = tag_name == "a";
    let is_block = is_block_element(tag_name);

    if is_block && !text_out.is_empty() && !text_out.ends_with(char::is_whitespace) {
        text_out.push(' ');
    }

    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            text_out.push_str(text);
            if is_link {
                *link_len_out += text.len();
            }
            continue;
        }
        if let Some(child_element) = ElementRef::wrap(child) {
            collect_block_stats(child_element, text_out, link_len_out);
        }
    }

    if is_block && !text_out.ends_with(char::is_whitespace) {
        text_out.push(' ');
    }
}

fn collect_raw_text(element: ElementRef<'_>, out: &mut String) {
    if is_non_content_element(element.value().name()) {
        return;
    }

    if is_block_element(element.value().name()) && !out.ends_with(char::is_whitespace) {
        out.push(' ');
    }

    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            out.push_str(text);
            continue;
        }
        if let Some(child_element) = ElementRef::wrap(child) {
            collect_raw_text(child_element, out);
        }
    }
}

fn is_block_element(tag: &str) -> bool {
    matches!(
        tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "br"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "td"
            | "th"
            | "tr"
            | "ul"
    )
}

fn is_non_content_element(tag: &str) -> bool {
    matches!(
        tag,
        "script"
            | "style"
            | "noscript"
            | "nav"
            | "header"
            | "footer"
            | "aside"
            | "menu"
            | "form"
            | "iframe"
            | "button"
    )
}

fn is_supported_html_content_type(content_type: &str) -> bool {
    let media_type = content_type
        .split_once(';')
        .map_or(content_type, |(media_type, _)| media_type)
        .trim();

    media_type.eq_ignore_ascii_case("text/html")
        || media_type.eq_ignore_ascii_case("application/xhtml+xml")
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use agent_kernel::{RunError, SourceFetcher};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use filetime::FileTime;
    use scraper::Html;
    use tokio::fs;

    use super::{
        DiskCacheSourceFetcher, SourceKind, SourceMaterial, extract_body_text,
        is_supported_html_content_type,
    };

    struct MockFetcher(Arc<AtomicUsize>);

    impl SourceFetcher for MockFetcher {
        fn fetch(
            &self,
            url: &str,
        ) -> agent_kernel::BoxFuture<'_, Result<SourceMaterial, RunError>> {
            let url = url.to_owned();
            let count = Arc::clone(&self.0);
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(SourceMaterial {
                    kind: SourceKind::UserUrl,
                    title: Some("Mock".to_owned()),
                    url,
                    summary: None,
                    content: "Fresh Content".to_owned(),
                })
            })
        }
    }

    #[test]
    fn body_extraction_preserves_word_boundaries_between_blocks() {
        let document = Html::parse_document("<html><body><p>Hello</p><p>World</p></body></html>");
        let body = extract_body_text(&document).unwrap_or_default();
        assert_eq!(body, "Hello World");
    }

    #[test]
    fn body_extraction_keeps_inline_word_contiguous() {
        let document =
            Html::parse_document("<html><body><p>exa<strong>mple</strong></p></body></html>");
        let body = extract_body_text(&document).unwrap_or_default();
        assert_eq!(body, "example");
    }

    #[test]
    fn html_content_type_check_is_case_insensitive() {
        assert!(is_supported_html_content_type("Text/HTML; charset=UTF-8"));
        assert!(is_supported_html_content_type("APPLICATION/XHTML+XML"));
        assert!(!is_supported_html_content_type("application/json"));
    }

    #[tokio::test]
    async fn disk_cache_enforces_expiration() -> Result<(), RunError> {
        let temp_dir = std::env::temp_dir().join(format!(
            "agent-cache-test-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_err(|error| RunError::Internal(error.to_string()))?
                .as_millis()
        ));
        let url = "https://example.com/stale";
        let call_count = Arc::new(AtomicUsize::new(0));
        let fetcher = DiskCacheSourceFetcher::new(MockFetcher(call_count.clone()), &temp_dir, 1);

        let res1 = fetcher.fetch(url).await?;
        assert_eq!(res1.content, "Fresh Content");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        let res2 = fetcher.fetch(url).await?;
        assert_eq!(res2.content, "Fresh Content");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        let cache_path = fetcher.cache_path(url);
        let stale_time = SystemTime::now() - Duration::from_hours(48);
        filetime::set_file_mtime(&cache_path, FileTime::from_system_time(stale_time))
            .map_err(|error| RunError::Internal(error.to_string()))?;

        let res3 = fetcher.fetch(url).await?;
        assert_eq!(res3.content, "Fresh Content");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);

        fs::remove_dir_all(temp_dir)
            .await
            .map_err(|error| RunError::Internal(error.to_string()))?;
        Ok(())
    }
}
