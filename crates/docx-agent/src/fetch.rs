use agent_core::{FetchedSource, SourceKind, UrlFetcher};
use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use scraper::{ElementRef, Html, Selector};
use tracing::{info, warn};

use crate::error::DocxAgentError;

#[derive(Debug, Clone)]
pub struct WebPageFetcher {
    client: reqwest::Client,
    max_chars: usize,
}

impl WebPageFetcher {
    pub fn new(user_agent: &str, max_chars: usize) -> Result<Self, DocxAgentError> {
        let client = reqwest::Client::builder().user_agent(user_agent).build()?;

        Ok(Self { client, max_chars })
    }
}

#[async_trait]
impl UrlFetcher for WebPageFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchedSource, agent_core::BoxError> {
        info!(url, "fetching user-provided URL");
        let response = self.client.get(url).send().await?.error_for_status()?;

        if let Some(content_type) = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|header| header.to_str().ok())
            && !is_supported_html_content_type(content_type)
        {
            warn!(url, content_type, "skipping unsupported URL content type");
            return Err(DocxAgentError::UnsupportedContentType(content_type.to_owned()).into());
        }

        let body = response.text().await?;
        let document = Html::parse_document(&body);
        let title = select_first_text(&document, "title");
        let content = extract_body_text(&document)
            .map(|text| truncate_chars(&text, self.max_chars))
            .unwrap_or_default();

        let source = FetchedSource {
            kind: SourceKind::UserUrl,
            title,
            url: url.to_owned(),
            summary: None,
            content,
        };

        info!(
            url,
            title = source.title.as_deref().unwrap_or(""),
            chars = source.content.chars().count(),
            "fetched user-provided URL"
        );

        Ok(source)
    }
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
    let mut raw = String::new();
    collect_body_text(body, &mut raw);
    let normalized = normalize_whitespace(&raw);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn collect_body_text(element: ElementRef<'_>, out: &mut String) {
    if is_non_content_element(element.value().name()) {
        return;
    }

    let is_block = is_block_element(element.value().name());
    if is_block && !out.ends_with(char::is_whitespace) {
        out.push(' ');
    }

    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            out.push_str(text);
            continue;
        }
        if let Some(child_element) = ElementRef::wrap(child) {
            collect_body_text(child_element, out);
        }
    }

    if is_block && !out.ends_with(char::is_whitespace) {
        out.push(' ');
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
    matches!(tag, "script" | "style" | "noscript")
}

fn is_supported_html_content_type(content_type: &str) -> bool {
    let media_type = content_type
        .split_once(';')
        .map_or(content_type, |(media_type, _)| media_type)
        .trim();

    media_type.eq_ignore_ascii_case("text/html")
        || media_type.eq_ignore_ascii_case("application/xhtml+xml")
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[cfg(test)]
mod tests {
    use super::{extract_body_text, is_supported_html_content_type};
    use scraper::Html;

    #[test]
    fn body_extraction_preserves_word_boundaries_between_blocks() {
        let document = Html::parse_document("<html><body><p>Hello</p><p>World</p></body></html>");
        let body = extract_body_text(&document).expect("body text should exist");
        assert_eq!(body, "Hello World");
    }

    #[test]
    fn body_extraction_keeps_inline_word_contiguous() {
        let document =
            Html::parse_document("<html><body><p>exa<strong>mple</strong></p></body></html>");
        let body = extract_body_text(&document).expect("body text should exist");
        assert_eq!(body, "example");
    }

    #[test]
    fn body_extraction_skips_non_content_tags() {
        let document = Html::parse_document(
            "<html><body><script>var ignored = true;</script><style>.ignored { color: red; }</style><noscript>ignored fallback</noscript><p>Main text</p></body></html>",
        );
        let body = extract_body_text(&document).expect("body text should exist");
        assert_eq!(body, "Main text");
    }

    #[test]
    fn html_content_type_check_is_case_insensitive() {
        assert!(is_supported_html_content_type("Text/HTML; charset=UTF-8"));
        assert!(is_supported_html_content_type("APPLICATION/XHTML+XML"));
        assert!(!is_supported_html_content_type("application/json"));
        assert!(!is_supported_html_content_type(
            "application/json; profile=text/html"
        ));
    }
}
