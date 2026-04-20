use std::{collections::HashSet, path::PathBuf, sync::Arc};

use agent_kernel::{
    BoxFuture, DocumentParser, QualityGate, RetryPolicy, RunError, RunRequest, SearchProvider,
    SourceFetcher, SourceMaterial, StepExecution, StepTransition, Workflow, WorkflowContext,
    WorkflowDefinition, WorkflowStep,
};
use serde::Deserialize;
use tokio::{sync::Semaphore, task::JoinSet};
use tracing::warn;

use crate::{
    DocxAttemptRecord, DocxDocumentParser, DocxDraft, DocxEvaluation, DocxExpandRequest,
    DocxFinalOutput, DocxPlan, DocxPromptContext, DocxPromptFormatter, DocxPromptTemplates,
    DocxResearchArtifacts, Document, TokenBudget,
};

const DOCUMENT_ARTIFACT: &str = "docx.document";
const PLAN_ARTIFACT: &str = "docx.plan";
const RESEARCH_ARTIFACT: &str = "docx.research";
const DRAFT_ARTIFACT: &str = "docx.draft";
const EVALUATION_ARTIFACT: &str = "docx.evaluation";
const QUALITY_GATE_ARTIFACT: &str = "quality_gate";
const ATTEMPTS_ARTIFACT: &str = "docx.attempts";
const FINAL_OUTPUT_ARTIFACT: &str = "docx.final_output";

#[derive(Debug, Clone)]
pub struct DocxWorkflowConfig {
    pub planner_model: Option<String>,
    pub writer_model: String,
    pub reviewer_model: String,
    pub min_score: u8,
    pub max_refinement_rounds: usize,
    pub search_max_results: usize,
    pub fetch_concurrency_limit: usize,
    pub search_hint_terms: Vec<String>,
    pub search_negation_terms: Vec<String>,
    pub prompt_templates: DocxPromptTemplates,
    pub token_budget: TokenBudget,
}

impl DocxWorkflowConfig {
    #[must_use]
    pub fn formatter(&self) -> DocxPromptFormatter {
        DocxPromptFormatter::new(self.prompt_templates.clone(), self.token_budget.clone())
    }
}

#[derive(Clone)]
pub struct DocxWorkflow {
    config: DocxWorkflowConfig,
    parser: DocxDocumentParser,
}

impl DocxWorkflow {
    #[must_use]
    pub fn new(config: DocxWorkflowConfig) -> Self {
        Self {
            config,
            parser: DocxDocumentParser,
        }
    }
}

impl Workflow for DocxWorkflow {
    fn id(&self) -> &'static str {
        "docx.expand"
    }

    fn build(&self, request: &RunRequest) -> Result<WorkflowDefinition, RunError> {
        let _: DocxExpandRequest = serde_json::from_value(request.input.clone()).map_err(|error| {
            RunError::Workflow(format!("invalid docx workflow input: {error}"))
        })?;
        let formatter = self.config.formatter();

        Ok(WorkflowDefinition::new(
            self.id(),
            vec![
                Arc::new(ParseDocumentStep {
                    parser: self.parser,
                }),
                Arc::new(PlanStep {
                    planner_model: self.config.planner_model.clone(),
                    formatter: formatter.clone(),
                    search_hint_terms: self.config.search_hint_terms.clone(),
                    search_negation_terms: self.config.search_negation_terms.clone(),
                    max_refinement_rounds: self.config.max_refinement_rounds,
                }),
                Arc::new(ResearchStep {
                    search_max_results: self.config.search_max_results,
                    fetch_concurrency_limit: self.config.fetch_concurrency_limit,
                }),
                Arc::new(GenerateStep {
                    writer_model: self.config.writer_model.clone(),
                    formatter: formatter.clone(),
                }),
                Arc::new(EvaluateStep {
                    reviewer_model: self.config.reviewer_model.clone(),
                    formatter: formatter.clone(),
                    min_score: self.config.min_score,
                }),
                Arc::new(FinalizeStep),
                Arc::new(RefineStep {
                    writer_model: self.config.writer_model.clone(),
                    formatter,
                }),
            ],
        )
        .with_retry_policy(RetryPolicy {
            gate_step: "evaluate",
            gate_artifact: QUALITY_GATE_ARTIFACT,
            retry_from_step: "refine",
            max_attempts: self.config.max_refinement_rounds,
        })
        .with_default_output_artifact(FINAL_OUTPUT_ARTIFACT))
    }
}

#[derive(Clone, Copy)]
struct ParseDocumentStep {
    parser: DocxDocumentParser,
}

impl WorkflowStep for ParseDocumentStep {
    fn id(&self) -> &'static str {
        "parse_document"
    }

    fn execute(&self, mut context: WorkflowContext) -> BoxFuture<'static, Result<StepExecution, RunError>> {
        let parser = self.parser;
        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let document = parser.parse_path(&PathBuf::from(request.document_path))?;
            context.insert_artifact(DOCUMENT_ARTIFACT, "docx.document", &document)?;
            Ok(StepExecution::continue_with(context))
        })
    }
}

#[derive(Clone)]
struct PlanStep {
    planner_model: Option<String>,
    formatter: DocxPromptFormatter,
    search_hint_terms: Vec<String>,
    search_negation_terms: Vec<String>,
    max_refinement_rounds: usize,
}

impl WorkflowStep for PlanStep {
    fn id(&self) -> &'static str {
        "plan"
    }

    fn execute(&self, mut context: WorkflowContext) -> BoxFuture<'static, Result<StepExecution, RunError>> {
        let planner_model = self.planner_model.clone();
        let formatter = self.formatter.clone();
        let hint_terms = self.search_hint_terms.clone();
        let negation_terms = self.search_negation_terms.clone();
        let max_refinement_rounds = self.max_refinement_rounds;

        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let document: Document = context.artifact(DOCUMENT_ARTIFACT)?;
            let llm = planner_model
                .as_deref()
                .map(|name| context.services.llm(name))
                .transpose()?;

            let plan = if let Some(llm) = llm {
                let prompt = formatter.planning_prompt(&request, &document);
                match llm.complete(&prompt).await {
                    Ok(response) => {
                        parse_llm_plan(&response, max_refinement_rounds).unwrap_or_else(|| {
                            warn!("planner response was invalid JSON; falling back to heuristics");
                            heuristic_plan(
                                &request,
                                &document,
                                &hint_terms,
                                &negation_terms,
                                max_refinement_rounds,
                            )
                        })
                    }
                    Err(error) => {
                        warn!(error = %error, "planner model failed; falling back to heuristics");
                        heuristic_plan(
                            &request,
                            &document,
                            &hint_terms,
                            &negation_terms,
                            max_refinement_rounds,
                        )
                    }
                }
            } else {
                heuristic_plan(
                    &request,
                    &document,
                    &hint_terms,
                    &negation_terms,
                    max_refinement_rounds,
                )
            };

            context.insert_artifact(PLAN_ARTIFACT, "docx.plan", &plan)?;
            Ok(StepExecution::continue_with(context))
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

fn parse_llm_plan(response: &str, max_refinement_rounds: usize) -> Option<DocxPlan> {
    let trimmed = response.trim();
    let json = if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[start..=end]
    } else {
        trimmed
    };

    let payload = serde_json::from_str::<PlannerPayload>(json).ok()?;
    let search_mode = match payload.search_mode.as_deref()?.to_ascii_lowercase().as_str() {
        "disabled" => crate::model::SearchMode::Disabled,
        "required" => crate::model::SearchMode::Required,
        _ => crate::model::SearchMode::Auto,
    };

    Some(DocxPlan {
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
    request: &DocxExpandRequest,
    document: &Document,
    hint_terms: &[String],
    negation_terms: &[String],
    max_refinement_rounds: usize,
) -> DocxPlan {
    let lower_prompt = request.prompt.to_ascii_lowercase();
    let prompt_disables_research = request.source_policy.disable_research
        || negation_terms
            .iter()
            .any(|term| lower_prompt.contains(&term.to_ascii_lowercase()));
    let prompt_requests_research = hint_terms
        .iter()
        .any(|term| lower_prompt.contains(&term.to_ascii_lowercase()));

    let search_mode = if prompt_disables_research {
        crate::model::SearchMode::Disabled
    } else if prompt_requests_research {
        crate::model::SearchMode::Required
    } else {
        crate::model::SearchMode::Auto
    };

    let mut search_queries = Vec::new();
    if !matches!(search_mode, crate::model::SearchMode::Disabled) {
        let mut query_parts = Vec::new();
        if let Some(title) = &document.title {
            query_parts.push(title.clone());
        }
        if !request.prompt.trim().is_empty() {
            query_parts.push(request.prompt.trim().to_owned());
        }
        if !query_parts.is_empty() {
            search_queries.push(query_parts.join(" "));
        }
    }

    DocxPlan {
        objective: document
            .title
            .clone()
            .unwrap_or_else(|| "扩写并完善 DOCX 文档".to_owned()),
        search_mode,
        search_queries,
        evaluation_focus: "事实准确性、结构完整性、表达清晰度".to_owned(),
        max_refinement_rounds,
    }
}

#[derive(Clone, Copy)]
struct ResearchStep {
    search_max_results: usize,
    fetch_concurrency_limit: usize,
}

impl WorkflowStep for ResearchStep {
    fn id(&self) -> &'static str {
        "research"
    }

    fn execute(&self, mut context: WorkflowContext) -> BoxFuture<'static, Result<StepExecution, RunError>> {
        let search_max_results = self.search_max_results;
        let fetch_concurrency_limit = self.fetch_concurrency_limit;
        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let plan: DocxPlan = context.artifact(PLAN_ARTIFACT)?;
            let fetcher = context.services.source_fetcher()?;
            let search_provider = context.services.search_provider();

            let user_sources =
                collect_user_sources(fetcher, &request.user_urls, fetch_concurrency_limit).await;
            let search_sources =
                collect_search_sources(search_provider, &plan, search_max_results).await?;

            let mut queries = plan.search_queries.clone();
            let mut sources = user_sources;
            sources.extend(search_sources);
            deduplicate_sources(&mut sources);
            queries.dedup();

            let research = DocxResearchArtifacts { queries, sources };
            context.insert_artifact(RESEARCH_ARTIFACT, "docx.research", &research)?;
            Ok(StepExecution::continue_with(context))
        })
    }
}

async fn collect_user_sources(
    fetcher: Arc<dyn SourceFetcher>,
    urls: &[String],
    concurrency_limit: usize,
) -> Vec<SourceMaterial> {
    if urls.is_empty() {
        return Vec::new();
    }

    let semaphore = Arc::new(Semaphore::new(concurrency_limit.max(1)));
    let mut set = JoinSet::new();

    for url in urls {
        let fetcher = Arc::clone(&fetcher);
        let semaphore = Arc::clone(&semaphore);
        let url = url.clone();
        set.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .map_err(|error| RunError::Internal(error.to_string()))?;
            fetcher.fetch(&url).await
        });
    }

    let mut sources = Vec::with_capacity(urls.len());
    while let Some(result) = set.join_next().await {
        match result {
            Ok(Ok(source)) => sources.push(source),
            Ok(Err(error)) => warn!(error = %error, "failed to fetch user URL; skipping"),
            Err(error) => warn!(error = %error, "user URL task failed"),
        }
    }

    sources
}

async fn collect_search_sources(
    search_provider: Option<Arc<dyn SearchProvider>>,
    plan: &DocxPlan,
    max_results: usize,
) -> Result<Vec<SourceMaterial>, RunError> {
    match plan.search_mode {
        crate::model::SearchMode::Disabled => Ok(Vec::new()),
        crate::model::SearchMode::Auto | crate::model::SearchMode::Required => {
            if plan.search_queries.is_empty() {
                return Ok(Vec::new());
            }

            let Some(search_provider) = search_provider else {
                if matches!(plan.search_mode, crate::model::SearchMode::Required) {
                    return Err(RunError::Workflow(
                        "search was required by the DOCX plan but no search provider is configured"
                            .to_owned(),
                    ));
                }
                return Ok(Vec::new());
            };

            let mut sources = Vec::new();
            for query in &plan.search_queries {
                match search_provider.search(query, max_results).await {
                    Ok(mut results) => sources.append(&mut results),
                    Err(error) => {
                        if matches!(plan.search_mode, crate::model::SearchMode::Required) {
                            return Err(error);
                        }
                        warn!(query, error = %error, "optional search query failed");
                    }
                }
            }

            Ok(sources)
        }
    }
}

fn deduplicate_sources(sources: &mut Vec<SourceMaterial>) {
    let mut seen = HashSet::new();
    sources.retain(|source| seen.insert(source.url.clone()));
}

#[derive(Clone)]
struct GenerateStep {
    writer_model: String,
    formatter: DocxPromptFormatter,
}

impl WorkflowStep for GenerateStep {
    fn id(&self) -> &'static str {
        "generate"
    }

    fn execute(&self, mut context: WorkflowContext) -> BoxFuture<'static, Result<StepExecution, RunError>> {
        let writer_model = self.writer_model.clone();
        let formatter = self.formatter.clone();
        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let document: Document = context.artifact(DOCUMENT_ARTIFACT)?;
            let plan: DocxPlan = context.artifact(PLAN_ARTIFACT)?;
            let research: DocxResearchArtifacts = context.artifact(RESEARCH_ARTIFACT)?;
            let llm = context.services.llm(&writer_model)?;
            let prompt_context = DocxPromptContext {
                request,
                document,
                plan,
                research,
            };
            let outline = llm
                .complete(&formatter.outline_prompt(&prompt_context))
                .await?;
            let markdown = llm
                .complete(&formatter.generation_prompt(&prompt_context, &outline))
                .await?;
            context.insert_artifact(
                DRAFT_ARTIFACT,
                "docx.draft",
                &DocxDraft {
                    content: markdown,
                    outline: Some(outline),
                },
            )?;
            Ok(StepExecution::continue_with(context))
        })
    }
}

#[derive(Clone)]
struct EvaluateStep {
    reviewer_model: String,
    formatter: DocxPromptFormatter,
    min_score: u8,
}

impl WorkflowStep for EvaluateStep {
    fn id(&self) -> &'static str {
        "evaluate"
    }

    fn execute(&self, mut context: WorkflowContext) -> BoxFuture<'static, Result<StepExecution, RunError>> {
        let reviewer_model = self.reviewer_model.clone();
        let formatter = self.formatter.clone();
        let min_score = self.min_score;
        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let document: Document = context.artifact(DOCUMENT_ARTIFACT)?;
            let plan: DocxPlan = context.artifact(PLAN_ARTIFACT)?;
            let research: DocxResearchArtifacts = context.artifact(RESEARCH_ARTIFACT)?;
            let draft: DocxDraft = context.artifact(DRAFT_ARTIFACT)?;
            let llm = context.services.llm(&reviewer_model)?;
            let prompt_context = DocxPromptContext {
                request,
                document,
                plan,
                research,
            };
            let response = llm
                .complete(&formatter.evaluation_prompt(&prompt_context, &draft.content))
                .await?;
            let mut evaluation = parse_evaluation_response(&response)?;
            evaluation.qualified = evaluation.score >= min_score;
            let gate: QualityGate = evaluation.clone().into();

            context.insert_artifact(EVALUATION_ARTIFACT, "docx.evaluation", &evaluation)?;
            context.insert_artifact(QUALITY_GATE_ARTIFACT, "quality_gate", &gate)?;

            let mut attempts = context
                .artifacts
                .get::<Vec<DocxAttemptRecord>>(ATTEMPTS_ARTIFACT)
                .unwrap_or_default();
            attempts.push(DocxAttemptRecord {
                attempt: context.attempt,
                draft,
                evaluation,
            });
            context.insert_artifact(ATTEMPTS_ARTIFACT, "docx.attempts", &attempts)?;

            Ok(StepExecution {
                context,
                transition: StepTransition::JumpTo("finalize"),
            })
        })
    }
}

#[derive(Debug, Deserialize)]
struct EvaluationPayload {
    score: u8,
    reason: String,
}

fn parse_evaluation_response(response: &str) -> Result<DocxEvaluation, RunError> {
    let trimmed = response.trim();
    let json = if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[start..=end]
    } else {
        trimmed
    };
    let payload: EvaluationPayload = serde_json::from_str(json)
        .map_err(|error| RunError::Evaluation(format!("invalid evaluation JSON: {error}")))?;
    Ok(DocxEvaluation {
        score: payload.score,
        reason: payload.reason,
        qualified: false,
    })
}

struct FinalizeStep;

impl WorkflowStep for FinalizeStep {
    fn id(&self) -> &'static str {
        "finalize"
    }

    fn execute(&self, mut context: WorkflowContext) -> BoxFuture<'static, Result<StepExecution, RunError>> {
        Box::pin(async move {
            let draft: DocxDraft = context.artifact(DRAFT_ARTIFACT)?;
            let evaluation: DocxEvaluation = context.artifact(EVALUATION_ARTIFACT)?;
            let output = DocxFinalOutput {
                markdown: draft.content,
                score: evaluation.score,
                qualified: evaluation.qualified,
                reason: evaluation.reason,
            };
            context.insert_artifact(FINAL_OUTPUT_ARTIFACT, "docx.final_output", &output)?;
            Ok(StepExecution {
                context,
                transition: StepTransition::Complete {
                    output_artifact: Some(FINAL_OUTPUT_ARTIFACT),
                    qualified: output.qualified,
                },
            })
        })
    }
}

#[derive(Clone)]
struct RefineStep {
    writer_model: String,
    formatter: DocxPromptFormatter,
}

impl WorkflowStep for RefineStep {
    fn id(&self) -> &'static str {
        "refine"
    }

    fn execute(&self, mut context: WorkflowContext) -> BoxFuture<'static, Result<StepExecution, RunError>> {
        let writer_model = self.writer_model.clone();
        let formatter = self.formatter.clone();
        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let document: Document = context.artifact(DOCUMENT_ARTIFACT)?;
            let plan: DocxPlan = context.artifact(PLAN_ARTIFACT)?;
            let research: DocxResearchArtifacts = context.artifact(RESEARCH_ARTIFACT)?;
            let draft: DocxDraft = context.artifact(DRAFT_ARTIFACT)?;
            let evaluation: DocxEvaluation = context.artifact(EVALUATION_ARTIFACT)?;
            let llm = context.services.llm(&writer_model)?;
            let prompt_context = DocxPromptContext {
                request,
                document,
                plan,
                research,
            };
            let refined = llm
                .complete(&formatter.refinement_prompt(
                    &prompt_context,
                    &draft.content,
                    &evaluation.reason,
                ))
                .await?;
            context.insert_artifact(
                DRAFT_ARTIFACT,
                "docx.draft",
                &DocxDraft {
                    content: refined,
                    outline: draft.outline,
                },
            )?;
            Ok(StepExecution {
                context,
                transition: StepTransition::JumpTo("evaluate"),
            })
        })
    }
}
