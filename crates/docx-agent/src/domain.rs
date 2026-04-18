use agent_core::{ExpansionRequest, FetchedSource};
use tiktoken_rs::cl100k_base;

pub(crate) fn truncate_tokens(text: &str, max_tokens: usize) -> String {
    if max_tokens == 0 {
        return "".to_owned();
    }
    let bpe = cl100k_base().expect("failed to load tiktoken bpe");
    let tokens = bpe.encode_with_special_tokens(text);
    if tokens.len() <= max_tokens {
        return text.to_owned();
    }
    bpe.decode(tokens[..max_tokens].to_vec())
        .unwrap_or_else(|_| text.chars().take(max_tokens * 4).collect())
}

pub(crate) fn count_tokens(text: &str) -> usize {
    let bpe = cl100k_base().expect("failed to load tiktoken bpe");
    bpe.encode_with_special_tokens(text).len()
}

pub struct ContextBudgeter {
    total_budget: usize,
}

impl ContextBudgeter {
    pub fn new(total_budget: usize) -> Self {
        Self { total_budget }
    }

    pub fn allocate_limits(
        &self,
        prompt: &str,
        outline: Option<&str>,
        sources_count: usize,
    ) -> (usize, usize) {
        let prompt_tokens = count_tokens(prompt);
        let outline_tokens = outline.map_or(0, count_tokens);

        // Reserve tokens for output, templates, and fixed fields (approx 2000 tokens)
        let reserved = prompt_tokens + outline_tokens + 2000;
        let available = self.total_budget.saturating_sub(reserved);

        if sources_count == 0 {
            return (available, 0);
        }

        // Allocate 50% to document, 50% to external sources
        let doc_limit = (available as f32 * 0.5) as usize;
        let sources_limit = available.saturating_sub(doc_limit);
        let per_source_limit = sources_limit / sources_count;

        (doc_limit, per_source_limit)
    }
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
    budgeter: &ContextBudgeter,
) -> String {
    let (doc_limit, source_limit) = budgeter.allocate_limits(&request.prompt, None, sources.len());

    let document_markdown = truncate_tokens(&request.document.render_markdown(), doc_limit);
    let source_sections = render_source_sections(sources, source_limit);
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
    budgeter: &ContextBudgeter,
) -> String {
    let (doc_limit, source_limit) =
        budgeter.allocate_limits(&request.prompt, Some(outline), sources.len());

    let document_markdown = truncate_tokens(&request.document.render_markdown(), doc_limit);
    let source_sections = render_source_sections(sources, source_limit);
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
