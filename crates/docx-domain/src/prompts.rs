use std::sync::OnceLock;

use agent_kernel::{Plan, ResearchArtifacts, Task, truncate_chars};
use tiktoken_rs::{CoreBPE, cl100k_base};

static TOKENIZER: OnceLock<CoreBPE> = OnceLock::new();

const SYSTEM_PROMPT_DEFAULT: &str = r"你是一个面向 DOCX 文档扩写与整理的中文写作助手。你必须严格基于给定文档、用户要求与外部资料完成任务；资料不足时要明确边界，不得编造事实或最新数据。输出中文 Markdown。";
const PLANNING_TEMPLATE_DEFAULT: &str = r#"你是工作流规划器。请基于用户任务决定是否需要外部研究，并返回 JSON。

任务:
{prompt}

文档:
{document}

用户 URL:
{user_urls}

返回 JSON:
{
  "objective": "一句话概括目标",
  "search_mode": "disabled|auto|required",
  "search_queries": ["query1"],
  "evaluation_focus": "评估重点"
}"#;
const OUTLINE_TEMPLATE_DEFAULT: &str = r"任务:
{prompt}

工作流目标:
{objective}

文档:
{document}

用户 URL:
{user_urls}

外部材料:
{sources}

请生成一个详细的中文 Markdown 大纲。";
const GENERATION_TEMPLATE_DEFAULT: &str = r"任务:
{prompt}

工作流目标:
{objective}

文档:
{document}

用户 URL:
{user_urls}

外部材料:
{sources}

扩写大纲:
{outline}

请直接输出最终中文 Markdown。";
const EVALUATION_TEMPLATE_DEFAULT: &str = r#"你是一位严苛的文档评审专家。请根据任务目标和参考资料对内容评分，并输出 JSON。

任务:
{prompt}

工作流目标:
{objective}

评估重点:
{evaluation_focus}

生成内容:
{content}

外部材料:
{sources}

返回 JSON:
{
  "score": 0,
  "reason": "评分理由"
}"#;
const REFINEMENT_TEMPLATE_DEFAULT: &str = r"任务:
{prompt}

工作流目标:
{objective}

上一版内容:
{content}

评审意见:
{reason}

外部材料:
{sources}

请根据评审意见直接输出优化后的中文 Markdown。";

fn get_tokenizer() -> &'static CoreBPE {
    TOKENIZER.get_or_init(|| cl100k_base().expect("failed to load tiktoken bpe"))
}

#[must_use]
pub fn count_tokens(text: &str) -> usize {
    get_tokenizer().encode_with_special_tokens(text).len()
}

fn truncate_tokens(text: &str, max_tokens: usize) -> String {
    if max_tokens == 0 {
        return String::new();
    }

    let tokens = get_tokenizer().encode_with_special_tokens(text);
    if tokens.len() <= max_tokens {
        return text.to_owned();
    }

    get_tokenizer()
        .decode(tokens[..max_tokens].to_vec())
        .unwrap_or_else(|_| truncate_chars(text, max_tokens * 4))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenBudget {
    pub document_tokens: usize,
    pub source_tokens: usize,
    pub total_tokens: usize,
}

impl TokenBudget {
    #[must_use]
    pub fn new(document_tokens: usize, source_tokens: usize, total_tokens: usize) -> Self {
        Self {
            document_tokens,
            source_tokens,
            total_tokens,
        }
    }
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

impl Default for DocxPromptTemplates {
    fn default() -> Self {
        Self {
            system: SYSTEM_PROMPT_DEFAULT.to_owned(),
            planning: PLANNING_TEMPLATE_DEFAULT.to_owned(),
            outline: OUTLINE_TEMPLATE_DEFAULT.to_owned(),
            generation: GENERATION_TEMPLATE_DEFAULT.to_owned(),
            evaluation: EVALUATION_TEMPLATE_DEFAULT.to_owned(),
            refinement: REFINEMENT_TEMPLATE_DEFAULT.to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DocxPromptContext {
    pub task: Task,
    pub plan: Plan,
    pub research: ResearchArtifacts,
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
    pub fn system_prompt(&self) -> &str {
        &self.templates.system
    }

    #[must_use]
    pub fn planning_prompt(&self, task: &Task) -> String {
        let user_urls = render_user_urls(&task.user_urls);
        let document = truncate_tokens(
            &task.document.render_markdown(),
            self.budget.document_tokens,
        );

        self.templates
            .planning
            .replace("{prompt}", &task.prompt)
            .replace("{document}", &document)
            .replace("{user_urls}", &user_urls)
    }

    #[must_use]
    pub fn outline_prompt(&self, context: &DocxPromptContext) -> String {
        let document = truncate_tokens(
            &context.task.document.render_markdown(),
            self.document_limit(None, context.research.sources.len()),
        );
        let sources = render_sources(
            &context.research,
            self.source_limit(None, context.research.sources.len()),
        );
        let user_urls = render_user_urls(&context.task.user_urls);

        self.templates
            .outline
            .replace("{prompt}", &context.task.prompt)
            .replace("{objective}", &context.plan.objective)
            .replace("{document}", &document)
            .replace("{user_urls}", &user_urls)
            .replace("{sources}", &sources)
    }

    #[must_use]
    pub fn generation_prompt(&self, context: &DocxPromptContext, outline: &str) -> String {
        let document = truncate_tokens(
            &context.task.document.render_markdown(),
            self.document_limit(Some(outline), context.research.sources.len()),
        );
        let sources = render_sources(
            &context.research,
            self.source_limit(Some(outline), context.research.sources.len()),
        );
        let user_urls = render_user_urls(&context.task.user_urls);

        self.templates
            .generation
            .replace("{prompt}", &context.task.prompt)
            .replace("{objective}", &context.plan.objective)
            .replace("{document}", &document)
            .replace("{user_urls}", &user_urls)
            .replace("{sources}", &sources)
            .replace("{outline}", outline)
    }

    #[must_use]
    pub fn evaluation_prompt(
        &self,
        prompt_context: &DocxPromptContext,
        draft_content: &str,
    ) -> String {
        let sources = render_sources(&prompt_context.research, self.budget.source_tokens);

        self.templates
            .evaluation
            .replace("{prompt}", &prompt_context.task.prompt)
            .replace("{objective}", &prompt_context.plan.objective)
            .replace("{evaluation_focus}", &prompt_context.plan.evaluation_focus)
            .replace("{content}", draft_content)
            .replace("{sources}", &sources)
    }

    #[must_use]
    pub fn refinement_prompt(
        &self,
        prompt_context: &DocxPromptContext,
        draft_content: &str,
        reason: &str,
    ) -> String {
        let sources = render_sources(&prompt_context.research, self.budget.source_tokens);

        self.templates
            .refinement
            .replace("{prompt}", &prompt_context.task.prompt)
            .replace("{objective}", &prompt_context.plan.objective)
            .replace("{content}", draft_content)
            .replace("{reason}", reason)
            .replace("{sources}", &sources)
    }

    fn document_limit(&self, outline: Option<&str>, sources_count: usize) -> usize {
        self.allocate_limits(outline, sources_count).0
    }

    fn source_limit(&self, outline: Option<&str>, sources_count: usize) -> usize {
        self.allocate_limits(outline, sources_count).1
    }

    fn allocate_limits(&self, outline: Option<&str>, sources_count: usize) -> (usize, usize) {
        let outline_tokens = outline.map_or(0, count_tokens);
        let reserved = (outline_tokens + 2000).min(self.budget.total_tokens / 4);
        let available = self.budget.total_tokens.saturating_sub(reserved);
        let doc_limit = self.budget.document_tokens.min(available / 2);

        if sources_count == 0 {
            return (doc_limit, 0);
        }

        let source_total = self
            .budget
            .source_tokens
            .min(available.saturating_sub(doc_limit));
        (doc_limit, source_total / sources_count)
    }
}

fn render_user_urls(user_urls: &[String]) -> String {
    if user_urls.is_empty() {
        "无".to_owned()
    } else {
        user_urls.join("\n")
    }
}

fn render_sources(research: &ResearchArtifacts, per_source_limit: usize) -> String {
    if research.sources.is_empty() {
        return "无".to_owned();
    }

    research
        .sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let content = truncate_tokens(&source.content, per_source_limit);
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

#[cfg(test)]
mod tests {
    use super::{DocxPromptFormatter, DocxPromptTemplates, TokenBudget};
    use agent_kernel::{Document, Plan, ResearchArtifacts, SearchMode, Task};

    #[test]
    fn formatter_preserves_small_budget_split() {
        let formatter = DocxPromptFormatter::new(
            DocxPromptTemplates::default(),
            TokenBudget::new(400, 300, 1000),
        );

        let prompt = formatter.generation_prompt(
            &super::DocxPromptContext {
                task: Task {
                    prompt: "扩写".to_owned(),
                    document: Document::default(),
                    user_urls: Vec::new(),
                    constraints: agent_kernel::RunConstraints::default(),
                },
                plan: Plan {
                    objective: "扩写文档".to_owned(),
                    search_mode: SearchMode::Auto,
                    search_queries: Vec::new(),
                    evaluation_focus: "真实性".to_owned(),
                    max_refinement_rounds: 2,
                },
                research: ResearchArtifacts::default(),
            },
            "大纲",
        );

        assert!(prompt.contains("扩写大纲"));
    }
}
