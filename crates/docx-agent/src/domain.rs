use agent_core::{ExpansionRequest, FetchedSource, truncate_chars};

pub(crate) fn render_generation_prompt(
    request: &ExpansionRequest,
    sources: &[FetchedSource],
    max_document_chars: usize,
) -> String {
    let document_markdown = truncate_chars(&request.document.render_markdown(), max_document_chars);
    let source_sections = if sources.is_empty() {
        "无".to_owned()
    } else {
        sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                format!(
                    "来源 {index}\n标题: {}\nURL: {}\n摘要: {}\n内容摘录:\n{}",
                    source.title.as_deref().unwrap_or("未提供"),
                    source.url,
                    source.summary.as_deref().unwrap_or("无"),
                    source.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    let user_urls = if request.user_urls.is_empty() {
        "无".to_owned()
    } else {
        request.user_urls.join("\n")
    };

    format!(
        "任务:\n{}\n\n文档:\n{}\n\n用户 URL:\n{}\n\n外部材料:\n{}\n\n请直接输出最终中文 Markdown。",
        request.prompt, document_markdown, user_urls, source_sections
    )
}
