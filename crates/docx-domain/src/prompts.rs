use agent_kernel::{normalize_whitespace, truncate_chars, SourceMaterial};
use crate::{Document, DocxExpandRequest};

#[must_use]
pub fn count_tokens(text: &str) -> usize {
    text.chars().count() / 4
}

#[derive(Debug, Clone)]
pub struct DocxPromptTemplates {
    pub system: String,
    pub planning: String,
    pub outline: String,
    pub generation: String,
    pub evaluation: String,
    pub refinement: String,
}

impl DocxPromptTemplates {
    #[must_use]
    pub fn default_zh() -> Self {
        Self {
            system: "你是一个专业的文档处理助手，擅长通过深入研究和逻辑推理来扩写和优化文档。".to_owned(),
            planning: "请根据用户需求和当前文档内容，制定一个扩写计划。".to_owned(),
            outline: "请基于以下研究资料和原文档，为扩写内容生成一个详细的大纲。\n\n文档内容：\n{document}\n\n研究资料：\n{sources}".to_owned(),
            generation: "请根据大纲扩写文档章节。\n\n大纲：\n{outline}\n\n研究资料：\n{sources}".to_owned(),
            evaluation: "请评估以下文档草稿的质量。\n\n目标：{objective}\n\n草稿内容：\n{draft}".to_owned(),
            refinement: "请根据评审意见修改文档。\n\n评审意见：{feedback}\n\n当前内容：\n{draft}".to_owned(),
        }
    }
}

impl Default for DocxPromptTemplates {
    fn default() -> Self {
        Self::default_zh()
    }
}

#[derive(Debug, Clone)]
pub struct TokenBudget {
    pub document_tokens: usize,
    pub source_tokens: usize,
    pub max_total_tokens: usize,
}

impl TokenBudget {
    #[must_use]
    pub fn new(document_tokens: usize, source_tokens: usize, max_total_tokens: usize) -> Self {
        Self {
            document_tokens,
            source_tokens,
            max_total_tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DocxPromptContext {
    pub request: DocxExpandRequest,
    pub document: Document,
    pub research_sources: Vec<SourceMaterial>,
}

#[derive(Debug, Clone)]
pub struct DocxPromptFormatter {
    templates: DocxPromptTemplates,
    budget: TokenBudget,
}

impl DocxPromptFormatter {
    #[must_use]
    pub fn new(templates: DocxPromptTemplates, budget: TokenBudget) -> Self {
        Self { templates, budget }
    }

    #[must_use]
    pub fn outline_prompt(&self, context: &DocxPromptContext) -> String {
        let sources = render_sources(&context.research_sources, self.budget.source_tokens);
        self.templates
            .outline
            .replace("{document}", &context.document.render_markdown())
            .replace("{sources}", &sources)
    }

    #[must_use]
    pub fn generation_prompt(&self, context: &DocxPromptContext, outline: &str) -> String {
        let sources = render_sources(&context.research_sources, self.budget.source_tokens);
        self.templates
            .generation
            .replace("{outline}", outline)
            .replace("{sources}", &sources)
    }

    #[must_use]
    pub fn evaluation_prompt(&self, context: &DocxPromptContext, draft: &str) -> String {
        self.templates
            .evaluation
            .replace("{objective}", &context.request.prompt)
            .replace("{draft}", draft)
    }

    #[must_use]
    pub fn refinement_prompt(
        &self,
        context: &DocxPromptContext,
        draft: &str,
        feedback: &str,
    ) -> String {
        self.templates
            .refinement
            .replace("{feedback}", feedback)
            .replace("{draft}", draft)
            .replace("{objective}", &context.request.prompt)
    }
}

fn render_sources(sources: &[SourceMaterial], token_limit: usize) -> String {
    let mut rendered = Vec::new();
    let mut current_tokens = 0;
    let char_limit = token_limit * 4;

    for source in sources {
        let entry = format!(
            "Source: {}\nURL: {}\nContent: {}\n---",
            source.title.as_deref().unwrap_or("Unknown"),
            source.url,
            normalize_whitespace(&source.content)
        );
        let entry_tokens = count_tokens(&entry);
        if current_tokens + entry_tokens > token_limit && !rendered.is_empty() {
            break;
        }
        rendered.push(entry);
        current_tokens += entry_tokens;
        if current_tokens >= token_limit {
            break;
        }
    }

    let mut final_str = rendered.join("\n\n");
    if final_str.chars().count() > char_limit {
        final_str = truncate_chars(&final_str, char_limit);
    }
    final_str
}
