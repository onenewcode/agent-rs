use agent_core::{ExpansionRequest, FetchedSource};
use tiktoken_rs::cl100k_base;

pub(crate) fn truncate_tokens(text: &str, max_tokens: usize) -> String {
    let bpe = cl100k_base().expect("failed to load tiktoken bpe");
    let tokens = bpe.encode_with_special_tokens(text);
    if tokens.len() <= max_tokens {
        return text.to_owned();
    }
    bpe.decode(tokens[..max_tokens].to_vec())
        .unwrap_or_else(|_| text.chars().take(max_tokens * 4).collect())
}

pub(crate) fn build_fallback_search_query(request: &ExpansionRequest) -> String {
    let mut parts = Vec::new();
    if let Some(title) = &request.document.title {
        parts.push(title.clone());
    }
    if !request.prompt.trim().is_empty() {
        parts.push(request.prompt.trim().to_owned());
    }
    parts.join(" ")
}

pub(crate) fn render_outline_prompt(
    template: &str,
    request: &ExpansionRequest,
    sources: &[FetchedSource],
    max_document_tokens: usize,
    max_source_tokens: usize,
) -> String {
    let document_markdown = truncate_tokens(&request.document.render_markdown(), max_document_tokens);
    let source_sections = render_source_sections(sources, max_source_tokens);
    let user_urls = if request.user_urls.is_empty() {
        "无".to_owned()
    } else {
        request.user_urls.join("\n")
    };

    template
        .replace("{prompt}", &request.prompt)
        .replace("{document}", &document_markdown)
        .replace("{user_urls}", &user_urls)
        .replace("{sources}", &source_sections)
}

pub(crate) fn render_generation_prompt(
    template: &str,
    request: &ExpansionRequest,
    sources: &[FetchedSource],
    outline: &str,
    max_document_tokens: usize,
    max_source_tokens: usize,
) -> String {
    let document_markdown = truncate_tokens(&request.document.render_markdown(), max_document_tokens);
    let source_sections = render_source_sections(sources, max_source_tokens);
    let user_urls = if request.user_urls.is_empty() {
        "无".to_owned()
    } else {
        request.user_urls.join("\n")
    };

    template
        .replace("{prompt}", &request.prompt)
        .replace("{document}", &document_markdown)
        .replace("{user_urls}", &user_urls)
        .replace("{sources}", &source_sections)
        .replace("{outline}", outline)
}

fn render_source_sections(sources: &[FetchedSource], max_source_tokens: usize) -> String {
    if sources.is_empty() {
        return "无".to_owned();
    }

    sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let content = truncate_tokens(&source.content, max_source_tokens);
            format!(
                "来源 {index}\n标题: {}\nURL: {}\n摘要: {}\n内容摘录:\n{}",
                source.title.as_deref().unwrap_or("未提供"),
                source.url,
                source.summary.as_deref().unwrap_or("无"),
                content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
