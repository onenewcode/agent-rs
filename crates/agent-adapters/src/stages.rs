use std::sync::Arc;

use agent_kernel::{
    Draft, Evaluation, Evaluator, Generator, LanguageModel, Plan, Planner, Refiner,
    ResearchArtifacts, RunError, SearchMode, Task,
};
use docx_domain::{DocxPromptContext, DocxPromptFormatter};
use serde::Deserialize;
use tracing::{info, warn};

use crate::ResearchConfig;

#[derive(Clone)]
pub struct DocxPlanner {
    llm: Option<Arc<dyn LanguageModel>>,
    formatter: DocxPromptFormatter,
    search_hint_terms: Vec<String>,
    search_negation_terms: Vec<String>,
    max_refinement_rounds: usize,
}

impl DocxPlanner {
    #[must_use]
    pub fn new(
        llm: Option<Arc<dyn LanguageModel>>,
        formatter: DocxPromptFormatter,
        research: &ResearchConfig,
        max_refinement_rounds: usize,
    ) -> Self {
        Self {
            llm,
            formatter,
            search_hint_terms: research.search_hint_terms.clone(),
            search_negation_terms: research.search_negation_terms.clone(),
            max_refinement_rounds,
        }
    }
}

impl Planner for DocxPlanner {
    fn plan(&self, task: Task) -> agent_kernel::BoxFuture<'_, Result<Plan, RunError>> {
        let llm = self.llm.clone();
        let formatter = self.formatter.clone();
        let hint_terms = self.search_hint_terms.clone();
        let negation_terms = self.search_negation_terms.clone();
        let max_refinement_rounds = self.max_refinement_rounds;

        Box::pin(async move {
            if let Some(llm) = llm {
                let prompt = formatter.planning_prompt(&task);
                match llm.complete(&prompt).await {
                    Ok(response) => {
                        if let Some(plan) = parse_llm_plan(&response, max_refinement_rounds) {
                            info!(
                                search_queries = plan.search_queries.len(),
                                "planner produced LLM-backed workflow plan"
                            );
                            return Ok(plan);
                        }
                        warn!(
                            "planner LLM response was not valid JSON, falling back to heuristic planning"
                        );
                    }
                    Err(error) => {
                        warn!(
                            error = %error,
                            "planner LLM failed, falling back to heuristic planning"
                        );
                    }
                }
            }

            Ok(heuristic_plan(
                &task,
                &hint_terms,
                &negation_terms,
                max_refinement_rounds,
            ))
        })
    }
}

#[derive(Debug, Deserialize)]
struct PlannerPayload {
    objective: Option<String>,
    search_mode: Option<String>,
    search_queries: Option<Vec<String>>,
    evaluation_focus: Option<String>,
}

fn parse_llm_plan(response: &str, max_refinement_rounds: usize) -> Option<Plan> {
    let trimmed = response.trim();
    let json = if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[start..=end]
    } else {
        trimmed
    };

    let payload = serde_json::from_str::<PlannerPayload>(json).ok()?;
    let search_mode = match payload
        .search_mode
        .as_deref()?
        .to_ascii_lowercase()
        .as_str()
    {
        "disabled" => SearchMode::Disabled,
        "required" => SearchMode::Required,
        _ => SearchMode::Auto,
    };

    Some(Plan {
        objective: payload
            .objective
            .unwrap_or_else(|| "扩写并提升文档完整性".to_owned()),
        search_mode,
        search_queries: payload.search_queries.unwrap_or_default(),
        evaluation_focus: payload
            .evaluation_focus
            .unwrap_or_else(|| "事实准确性、结构完整性、表达清晰度".to_owned()),
        max_refinement_rounds,
    })
}

fn heuristic_plan(
    task: &Task,
    hint_terms: &[String],
    negation_terms: &[String],
    max_refinement_rounds: usize,
) -> Plan {
    let lower_prompt = task.prompt.to_ascii_lowercase();
    let prompt_disables_research = task.constraints.disable_research
        || negation_terms
            .iter()
            .any(|term| lower_prompt.contains(&term.to_ascii_lowercase()));
    let prompt_requests_research = hint_terms
        .iter()
        .any(|term| lower_prompt.contains(&term.to_ascii_lowercase()));

    let search_mode = if prompt_disables_research {
        SearchMode::Disabled
    } else if prompt_requests_research {
        SearchMode::Required
    } else {
        SearchMode::Auto
    };

    let mut search_queries = Vec::new();
    if search_mode != SearchMode::Disabled {
        let mut query_parts = Vec::new();
        if let Some(title) = &task.document.title {
            query_parts.push(title.clone());
        }
        if !task.prompt.trim().is_empty() {
            query_parts.push(task.prompt.trim().to_owned());
        }
        if !query_parts.is_empty() {
            search_queries.push(query_parts.join(" "));
        }
    }

    Plan {
        objective: task
            .document
            .title
            .clone()
            .unwrap_or_else(|| "扩写并完善 DOCX 文档".to_owned()),
        search_mode,
        search_queries,
        evaluation_focus: "事实准确性、结构完整性、表达清晰度".to_owned(),
        max_refinement_rounds,
    }
}

#[derive(Clone)]
pub struct DocxGenerator {
    llm: Arc<dyn LanguageModel>,
    formatter: DocxPromptFormatter,
}

impl DocxGenerator {
    #[must_use]
    pub fn new(llm: Arc<dyn LanguageModel>, formatter: DocxPromptFormatter) -> Self {
        Self { llm, formatter }
    }
}

impl Generator for DocxGenerator {
    fn generate(
        &self,
        task: Task,
        plan: Plan,
        research: ResearchArtifacts,
    ) -> agent_kernel::BoxFuture<'_, Result<Draft, RunError>> {
        let llm = Arc::clone(&self.llm);
        let formatter = self.formatter.clone();

        Box::pin(async move {
            let prompt_context = DocxPromptContext {
                task,
                plan,
                research,
            };
            let outline = llm
                .complete(&formatter.outline_prompt(&prompt_context))
                .await?;
            let generated_markdown = llm
                .complete(&formatter.generation_prompt(&prompt_context, &outline))
                .await?;
            Ok(Draft {
                content: generated_markdown,
                outline: Some(outline),
            })
        })
    }
}

#[derive(Clone)]
pub struct DocxEvaluator {
    llm: Arc<dyn LanguageModel>,
    formatter: DocxPromptFormatter,
}

impl DocxEvaluator {
    #[must_use]
    pub fn new(llm: Arc<dyn LanguageModel>, formatter: DocxPromptFormatter) -> Self {
        Self { llm, formatter }
    }
}

impl Evaluator for DocxEvaluator {
    fn evaluate(
        &self,
        task: Task,
        plan: Plan,
        research: ResearchArtifacts,
        draft: Draft,
    ) -> agent_kernel::BoxFuture<'_, Result<Evaluation, RunError>> {
        let llm = Arc::clone(&self.llm);
        let formatter = self.formatter.clone();

        Box::pin(async move {
            let prompt_context = DocxPromptContext {
                task,
                plan,
                research,
            };
            let response = llm
                .complete(&formatter.evaluation_prompt(&prompt_context, &draft.content))
                .await?;
            parse_evaluation_response(&response)
        })
    }
}

#[derive(Debug, Deserialize)]
struct EvaluationPayload {
    score: u8,
    reason: String,
}

fn parse_evaluation_response(response: &str) -> Result<Evaluation, RunError> {
    let trimmed = response.trim();
    let json = if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[start..=end]
    } else {
        trimmed
    };
    let payload: EvaluationPayload = serde_json::from_str(json)
        .map_err(|error| RunError::Evaluation(format!("invalid evaluation JSON: {error}")))?;
    Ok(Evaluation {
        score: payload.score,
        reason: payload.reason,
        qualified: false,
    })
}

#[derive(Clone)]
pub struct DocxRefiner {
    llm: Arc<dyn LanguageModel>,
    formatter: DocxPromptFormatter,
}

impl DocxRefiner {
    #[must_use]
    pub fn new(llm: Arc<dyn LanguageModel>, formatter: DocxPromptFormatter) -> Self {
        Self { llm, formatter }
    }
}

impl Refiner for DocxRefiner {
    fn refine(
        &self,
        task: Task,
        plan: Plan,
        research: ResearchArtifacts,
        draft: Draft,
        evaluation: Evaluation,
    ) -> agent_kernel::BoxFuture<'_, Result<Draft, RunError>> {
        let llm = Arc::clone(&self.llm);
        let formatter = self.formatter.clone();

        Box::pin(async move {
            let prompt_context = DocxPromptContext {
                task,
                plan,
                research,
            };
            let refined_markdown = llm
                .complete(&formatter.refinement_prompt(
                    &prompt_context,
                    &draft.content,
                    &evaluation.reason,
                ))
                .await?;
            Ok(Draft {
                content: refined_markdown,
                outline: draft.outline,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use agent_kernel::{Document, LanguageModel, Planner, RunConstraints, RunError};
    use docx_domain::{DocxPromptFormatter, DocxPromptTemplates, TokenBudget};

    use super::{DocxPlanner, parse_evaluation_response};

    struct StaticModel;

    impl LanguageModel for StaticModel {
        fn complete(&self, _prompt: &str) -> agent_kernel::BoxFuture<'_, Result<String, RunError>> {
            Box::pin(async {
                Ok(r#"{"objective":"扩写","search_mode":"required","search_queries":["query"],"evaluation_focus":"准确性"}"#.to_owned())
            })
        }
    }

    #[tokio::test]
    async fn planner_can_parse_llm_json() {
        let planner = DocxPlanner::new(
            Some(Arc::new(StaticModel)),
            DocxPromptFormatter::new(
                DocxPromptTemplates::default(),
                TokenBudget::new(100, 100, 1000),
            ),
            &crate::ResearchConfig {
                max_search_results: 5,
                fetch_concurrency_limit: 5,
                search_hint_terms: vec!["搜索".to_owned()],
                search_negation_terms: vec!["不要搜索".to_owned()],
            },
            2,
        );

        let plan = planner
            .plan(agent_kernel::Task {
                prompt: "扩写并搜索资料".to_owned(),
                document: Document::default(),
                user_urls: Vec::new(),
                constraints: RunConstraints::default(),
            })
            .await
            .expect("planner should succeed");

        assert_eq!(plan.search_queries, vec!["query"]);
    }

    #[test]
    fn evaluator_parses_json_payload() {
        let evaluation = parse_evaluation_response(r#"{"score":88,"reason":"ok"}"#)
            .expect("evaluation should parse");
        assert_eq!(evaluation.score, 88);
        assert_eq!(evaluation.reason, "ok");
    }
}
