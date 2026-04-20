use std::time::Duration;

use agent_kernel::{
    RunError, SourceFetcher, SourceKind, SourceMaterial, truncate_chars,
};
use reqwest::header::CONTENT_TYPE;
use scraper::{ElementRef, Html, Selector};
use tokio::time::timeout;

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

fn select_first_text(document: &Html, selector: &str) -> Option<String> {
    Selector::parse(selector).ok().and_then(|selector| {
        document
            .select(&selector)
            .map(|node| node.text().collect::<String>().split_whitespace().collect::<Vec<_>>().join(" "))
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
            let density = total_len as f64 / (link_len as f64 + 1.0);
            total_len > 40 && density > 2.0
        })
        .map(|block| block.text)
        .collect();

    if high_density_blocks.is_empty() {
        let mut raw = String::new();
        collect_raw_text(body, &mut raw);
        let normalized = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        return if normalized.is_empty() {
            None
        } else {
            Some(normalized)
        };
    }

    Some(high_density_blocks.join(" ").split_whitespace().collect::<Vec<_>>().join(" "))
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
    use scraper::Html;
    use super::{extract_body_text, is_supported_html_content_type};

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
}
