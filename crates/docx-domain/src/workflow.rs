use std::{collections::HashSet, path::PathBuf, sync::Arc};

use agent_kernel::{
    BoxFuture, DocumentParser, QualityGate, RunError, RunRequest, SearchProvider,
    SourceFetcher, SourceMaterial, StepConfig, StepTransition, Workflow, WorkflowContext,
    WorkflowDefinition, WorkflowStep,
};
use rig::completion::Prompt;
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
            "parse_document",
            vec![
                StepConfig::new(Arc::new(ParseDocumentStep {
                    parser: self.parser,
                })),
                StepConfig::new(Arc::new(PlanStep {
                    planner_model: self.config.planner_model.clone(),
                    formatter: formatter.clone(),
                    search_hint_terms: self.config.search_hint_terms.clone(),
                    search_negation_terms: self.config.search_negation_terms.clone(),
                    max_refinement_rounds: self.config.max_refinement_rounds,
                })).with_retry(2, 500),
                StepConfig::new(Arc::new(ResearchStep {
                    search_max_results: self.config.search_max_results,
                    fetch_concurrency_limit: self.config.fetch_concurrency_limit,
                })).with_retry(3, 1000),
                StepConfig::new(Arc::new(GenerateStep {
                    writer_model: self.config.writer_model.clone(),
                    formatter: formatter.clone(),
                })).with_retry(2, 1000),
                StepConfig::new(Arc::new(EvaluateStep {
                    reviewer_model: self.config.reviewer_model.clone(),
                    formatter: formatter.clone(),
                    min_score: self.config.min_score,
                    max_attempts: self.config.max_refinement_rounds,
                })).with_retry(2, 500),
                StepConfig::new(Arc::new(FinalizeStep)),
                StepConfig::new(Arc::new(RefineStep {
                    writer_model: self.config.writer_model.clone(),
                    formatter,
                })).with_retry(2, 1000),
            ],
        ))
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

    fn execute<'a>(&self, context: &'a mut WorkflowContext) -> BoxFuture<'a, Result<StepTransition, RunError>> {
        let parser = self.parser;
        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let document = parser.parse_path(&PathBuf::from(request.document_path))?;
            context.emit_artifact(DOCUMENT_ARTIFACT, "docx.document", &document)?;
            context.insert_state(document);
            Ok(StepTransition::Next("plan"))
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

    fn execute<'a>(&self, context: &'a mut WorkflowContext) -> BoxFuture<'a, Result<StepTransition, RunError>> {
        let planner_model = self.planner_model.clone();
        let formatter = self.formatter.clone();
        let hint_terms = self.search_hint_terms.clone();
        let negation_terms = self.search_negation_terms.clone();
        let max_refinement_rounds = self.max_refinement_rounds;

        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let document: Document = context.state::<Document>()?.clone();
            let llm = planner_model
                .as_deref()
                .map(|name| context.services.llm(name))
                .transpose()?;

            let plan = if let Some(llm) = llm {
                let prompt = formatter.planning_prompt(&request, &document);
                match llm.complete(context, &prompt).await {
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

            context.emit_artifact(PLAN_ARTIFACT, "docx.plan", &plan)?;
            context.insert_state(plan);
            Ok(StepTransition::Next("research"))
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

    fn execute<'a>(&self, context: &'a mut WorkflowContext) -> BoxFuture<'a, Result<StepTransition, RunError>> {
        let search_max_results = self.search_max_results;
        let fetch_concurrency_limit = self.fetch_concurrency_limit;
        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let plan: DocxPlan = context.state::<DocxPlan>()?.clone();
            let fetcher = context.services.source_fetcher()?;
            let search_provider = context.services.search_provider();

            let (user_sources, search_sources_res) = tokio::join!(
                collect_user_sources(fetcher, &request.user_urls, fetch_concurrency_limit),
                collect_search_sources(search_provider, &plan, search_max_results, fetch_concurrency_limit)
            );

            let search_sources = search_sources_res?;
            let mut queries = plan.search_queries.clone();
            let mut sources = user_sources;
            sources.extend(search_sources);
            deduplicate_sources(&mut sources);
            queries.dedup();

            let research = DocxResearchArtifacts { queries, sources };
            context.emit_artifact(RESEARCH_ARTIFACT, "docx.research", &research)?;
            context.insert_state(research);
            Ok(StepTransition::Next("generate"))
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
    concurrency_limit: usize,
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

            let semaphore = Arc::new(Semaphore::new(concurrency_limit.max(1)));
            let mut set = JoinSet::new();

            for query in &plan.search_queries {
                let search_provider = Arc::clone(&search_provider);
                let semaphore = Arc::clone(&semaphore);
                let query = query.clone();
                let search_mode = plan.search_mode.clone();

                set.spawn(async move {
                    let _permit = semaphore
                        .acquire_owned()
                        .await
                        .map_err(|error| RunError::Internal(error.to_string()))?;

                    search_provider
                        .search(&query, max_results)
                        .await
                        .map_err(|error| {
                            if matches!(search_mode, crate::model::SearchMode::Required) {
                                error
                            } else {
                                warn!(query, error = %error, "optional search query failed");
                                RunError::Internal("ignoring optional search failure".to_owned())
                            }
                        })
                });
            }

            let mut sources = Vec::new();
            while let Some(result) = set.join_next().await {
                match result {
                    Ok(Ok(mut results)) => sources.append(&mut results),
                    Ok(Err(_)) => {} // Already warned above if optional, or it was Required and returned the error
                    Err(error) => warn!(error = %error, "search query task failed"),
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

    fn execute<'a>(&self, context: &'a mut WorkflowContext) -> BoxFuture<'a, Result<StepTransition, RunError>> {
        let writer_model = self.writer_model.clone();
        let formatter = self.formatter.clone();
        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let document: Document = context.state::<Document>()?.clone();
            let plan: DocxPlan = context.state::<DocxPlan>()?.clone();
            let research: DocxResearchArtifacts = context.state::<DocxResearchArtifacts>()?.clone();
            let llm = context.services.llm(&writer_model)?;
            let prompt_context = DocxPromptContext {
                request,
                document,
                plan,
                research,
            };
            let outline = llm
                .complete(context, &formatter.outline_prompt(&prompt_context))
                .await?;
            let markdown = llm
                .complete(context, &formatter.generation_prompt(&prompt_context, &outline))
                .await?;
            let draft = DocxDraft {
                content: markdown,
                outline: Some(outline),
            };
            context.emit_artifact(DRAFT_ARTIFACT, "docx.draft", &draft)?;
            context.insert_state(draft);
            Ok(StepTransition::Next("evaluate"))
        })
    }
}

#[derive(Clone)]
struct EvaluateStep {
    reviewer_model: String,
    formatter: DocxPromptFormatter,
    min_score: u8,
    max_attempts: usize,
}

struct RefinementCounter(usize);

impl WorkflowStep for EvaluateStep {
    fn id(&self) -> &'static str {
        "evaluate"
    }

    fn execute<'a>(&self, context: &'a mut WorkflowContext) -> BoxFuture<'a, Result<StepTransition, RunError>> {
        let reviewer_model = self.reviewer_model.clone();
        let formatter = self.formatter.clone();
        let min_score = self.min_score;
        let max_attempts = self.max_attempts;

        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let document: Document = context.state::<Document>()?.clone();
            let plan: DocxPlan = context.state::<DocxPlan>()?.clone();
            let research: DocxResearchArtifacts = context.state::<DocxResearchArtifacts>()?.clone();
            let draft: DocxDraft = context.state::<DocxDraft>()?.clone();
            let llm = context.services.llm(&reviewer_model)?;
            let prompt_context = DocxPromptContext {
                request,
                document,
                plan,
                research,
            };
            let response = llm
                .complete(context, &formatter.evaluation_prompt(&prompt_context, &draft.content))
                .await?;
            let mut evaluation = parse_evaluation_response(&response)?;
            // Qualification: Score threshold + minimum quality in key dimensions
            evaluation.qualified = evaluation.score >= min_score 
                && evaluation.faithfulness_score >= 3
                && evaluation.relevance_score >= 3;
            let gate: QualityGate = evaluation.clone().into();

            context.emit_artifact(EVALUATION_ARTIFACT, "docx.evaluation", &evaluation)?;
            context.emit_artifact(QUALITY_GATE_ARTIFACT, "quality_gate", &gate)?;
            context.insert_state(evaluation.clone());

            let mut attempts = context
                .state::<Vec<DocxAttemptRecord>>()
                .cloned()
                .unwrap_or_default();
            
            let current_attempt = context.state::<RefinementCounter>().map_or(0, |c| c.0);

            attempts.push(DocxAttemptRecord {
                attempt: current_attempt,
                draft,
                evaluation: evaluation.clone(),
            });
            context.insert_state(attempts.clone());
            context.emit_artifact(ATTEMPTS_ARTIFACT, "docx.attempts", &attempts)?;

            if evaluation.qualified || current_attempt >= max_attempts {
                Ok(StepTransition::Next("finalize"))
            } else {
                context.insert_state(RefinementCounter(current_attempt + 1));
                Ok(StepTransition::Next("refine"))
            }
        })
    }
}

#[derive(Debug, Deserialize)]
struct EvaluationPayload {
    score: u8,
    reason: String,
    #[serde(default)]
    faithfulness_score: u8,
    #[serde(default)]
    relevance_score: u8,
    #[serde(default)]
    accuracy_score: u8,
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
        faithfulness_score: payload.faithfulness_score,
        relevance_score: payload.relevance_score,
        accuracy_score: payload.accuracy_score,
    })
}

struct FinalizeStep;

impl WorkflowStep for FinalizeStep {
    fn id(&self) -> &'static str {
        "finalize"
    }

    fn execute<'a>(&self, context: &'a mut WorkflowContext) -> BoxFuture<'a, Result<StepTransition, RunError>> {
        Box::pin(async move {
            let draft: DocxDraft = context.state::<DocxDraft>()?.clone();
            let evaluation: DocxEvaluation = context.state::<DocxEvaluation>()?.clone();
            let output = DocxFinalOutput {
                markdown: draft.content,
                score: evaluation.score,
                qualified: evaluation.qualified,
                reason: evaluation.reason,
            };
            context.emit_artifact(FINAL_OUTPUT_ARTIFACT, "docx.final_output", &output)?;
            let output_artifact = context.artifacts.last().cloned();
            Ok(StepTransition::Complete {
                output_artifact,
                qualified: output.qualified,
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

    fn execute<'a>(&self, context: &'a mut WorkflowContext) -> BoxFuture<'a, Result<StepTransition, RunError>> {
        let writer_model = self.writer_model.clone();
        let formatter = self.formatter.clone();
        Box::pin(async move {
            let request: DocxExpandRequest = context.input_as()?;
            let document: Document = context.state::<Document>()?.clone();
            let plan: DocxPlan = context.state::<DocxPlan>()?.clone();
            let research: DocxResearchArtifacts = context.state::<DocxResearchArtifacts>()?.clone();
            let draft: DocxDraft = context.state::<DocxDraft>()?.clone();
            let evaluation: DocxEvaluation = context.state::<DocxEvaluation>()?.clone();
            let llm = context.services.llm(&writer_model)?;
            let search_provider = context.services.search_provider();
            let trajectory = context.state::<agent_kernel::AgentTrajectory>()?.clone();
            let trajectory_wrapper = Arc::new(tokio::sync::Mutex::new(trajectory));

            let prompt_context = DocxPromptContext {
                request,
                document,
                plan,
                research,
            };

            // Use tool-enabled agent for refinement
            let content_wrapper = Arc::new(tokio::sync::RwLock::new(draft.content.clone()));
            let edit_tool = agent_tools::EditDocumentTool {
                current_content: Arc::clone(&content_wrapper),
                trajectory: Arc::clone(&trajectory_wrapper),
            };

            let mut builder = llm.agent_builder().tool(edit_tool);

            if let Some(search) = search_provider {
                builder = builder.tool(agent_tools::WebSearchTool {
                    provider: search,
                    trajectory: Arc::clone(&trajectory_wrapper),
                });
            }

            let agent = builder.build();

            let refinement_prompt = formatter.refinement_prompt(
                &prompt_context,
                &draft.content,
                &evaluation.reason,
            );

            // The agent will call tools to modify content_wrapper
            let _response = agent
                .prompt(&refinement_prompt)
                .await
                .map_err(|e| RunError::Provider(e.to_string()))?;

            let refined_content = content_wrapper.read().await.clone();
            let traj = trajectory_wrapper.lock().await.clone();
            context.insert_state(traj);

            let refined_draft = DocxDraft {
                content: refined_content,
                outline: draft.outline,
            };

            context.emit_artifact(DRAFT_ARTIFACT, "docx.draft", &refined_draft)?;
            context.insert_state(refined_draft);
            Ok(StepTransition::Next("evaluate"))
        })
    }
}
