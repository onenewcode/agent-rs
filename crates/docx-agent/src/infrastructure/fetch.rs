use agent_core::{
    BoxFuture, FetchedSource, SourceKind, UrlFetcher, normalize_whitespace, truncate_chars,
};
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
    #[must_use]
    pub fn new(client: reqwest::Client, max_chars: usize) -> Self {
        Self { client, max_chars }
    }

    pub async fn fetch_url(&self, url: &str) -> Result<FetchedSource, DocxAgentError> {
        info!(url, "fetching user-provided URL");
        let response = self.client.get(url).send().await?.error_for_status()?;

        if let Some(content_type) = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|header| header.to_str().ok())
            && !is_supported_html_content_type(content_type)
        {
            warn!(url, content_type, "skipping unsupported URL content type");
            return Err(DocxAgentError::UnsupportedContentType(
                content_type.to_owned(),
            ));
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

impl UrlFetcher for WebPageFetcher {
    fn fetch(&self, url: &str) -> BoxFuture<'_, Result<FetchedSource, agent_core::ExpansionError>> {
        let url = url.to_owned();
        Box::pin(async move {
            self.fetch_url(&url)
                .await
                .map_err(|e| agent_core::ExpansionError::Network(e.to_string()))
        })
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

    let mut blocks = Vec::new();
    find_content_blocks(body, &mut blocks);

    // Score blocks based on text length and link density
    let high_density_blocks: Vec<String> = blocks
        .into_iter()
        .filter(|block| {
            let total_len = block.text.len();
            let link_len = block.link_text_len;
            #[allow(clippy::cast_precision_loss)]
            let density = total_len as f32 / (link_len as f32 + 1.0);

            // Heuristic: Keep blocks with substantial text and low link ratio
            total_len > 40 && density > 2.0
        })
        .map(|block| block.text)
        .collect();

    if high_density_blocks.is_empty() {
        // Fallback to simple extraction if density filtering was too aggressive
        let mut raw = String::new();
        collect_raw_text(body, &mut raw);
        let normalized = normalize_whitespace(&raw);
        return if normalized.is_empty() { None } else { Some(normalized) };
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
        
        // Even if this is a block, continue searching children for more specific blocks
        for child in element.children() {
            if let Some(child_element) = ElementRef::wrap(child) {
                find_content_blocks(child_element, out);
            }
        }
    } else {
        for child in element.children() {
            if let Some(child_element) = ElementRef::wrap(child) {
                find_content_blocks(child_element, out);
            }
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
    use super::{extract_body_text, is_supported_html_content_type};
    use scraper::Html;

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
    fn body_extraction_skips_non_content_tags() {
        let document = Html::parse_document(
            "<html><body><script>var ignored = true;</script><style>.ignored { color: red; }</style><noscript>ignored fallback</noscript><p>Main text</p></body></html>",
        );
        let body = extract_body_text(&document).unwrap_or_default();
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
