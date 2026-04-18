use agent_core::{ExpansionRequest, FetchedSource, truncate_chars};

pub(crate) fn build_search_query(request: &ExpansionRequest) -> String {
    let mut parts = Vec::new();
    if let Some(title) = &request.document.title {
        parts.push(title.clone());
    }
    if !request.prompt.trim().is_empty() {
        parts.push(request.prompt.trim().to_owned());
    }
    parts.join(" ")
}

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

#[cfg(test)]
mod tests {
    use super::build_search_query;
    use agent_core::{ExpansionRequest, ParsedDocument};

    #[test]
    fn search_query_prefers_document_title_and_prompt() {
        let request = ExpansionRequest {
            prompt: "补充市场数据".to_owned(),
            document: ParsedDocument {
                title: Some("智能写作方案".to_owned()),
                blocks: vec![],
            },
            user_urls: vec![],
        };
        assert_eq!(build_search_query(&request), "智能写作方案 补充市场数据");
    }
}
