use crate::prompts::WriterTemplates;
use agent_kernel::{
    AgentError, AuditorFeedbackList, AutonomousAgent, BoxFuture, ErrorType, FeedbackHistory,
    Result, SearchProvider, SourceFetcher, StepOutcome, TaskGoal, TrajectoryStep,
    WorkflowContext, truncate_chars,
};
use agent_tools::{EditDocumentTool, FetchUrlTool, WebSearchTool};
use agent_rig::OpenRouterRigModel;
use rig::completion::Prompt;
use std::sync::Arc;
use std::time::Instant;

pub struct DocumentWriter {
    llm: Arc<OpenRouterRigModel>,
    search: Arc<dyn SearchProvider>,
    fetcher: Arc<dyn SourceFetcher>,
}

impl DocumentWriter {
    #[must_use]
    pub fn new(
        llm: Arc<OpenRouterRigModel>,
        search: Arc<dyn SearchProvider>,
        fetcher: Arc<dyn SourceFetcher>,
    ) -> Self {
        Self {
            llm,
            search,
            fetcher,
        }
    }

    fn build_agent(
        &self,
        context: Arc<WorkflowContext>,
    ) -> rig::agent::Agent<impl rig::completion::CompletionModel> {
        self.llm
            .agent_builder()
            .preamble(
                "You are an autonomous document research and writing agent. \
                You have access to tools to search the web, fetch specific URLs, and edit the document surgically. \
                \
                CRITICAL INSTRUCTIONS:\n\
                1. If you need more information, use `web_search` or `fetch_url` autonomously.\n\
                2. If a tool returns an error (like 404), do not stop; use other search results or your internal knowledge to proceed.\n\
                3. To fix issues without rewriting everything, use `edit_document` to replace specific text segments. \n\
                4. If you used `edit_document` for all your changes, your FINAL response MUST BE EXACTLY 'DONE_EDITING'. \n\
                5. If you choose to rewrite the whole document, wrap the text in <expanded_document> tags. \n\
                6. Do not output conversational filler outside the tagged block if outputting the full document."
            )
            .tool(WebSearchTool {
                provider: self.search.clone(),
                context: context.clone(),
            })
            .tool(FetchUrlTool {
                fetcher: self.fetcher.clone(),
                context: context.clone(),
            })
            .tool(EditDocumentTool {
                context: context.clone(),
            })
            .default_max_turns(10)
            .build()
    }
}

impl AutonomousAgent for DocumentWriter {
    fn role(&self) -> &'static str {
        "Writer"
    }

    fn run<'a>(&'a self, context: Arc<WorkflowContext>) -> BoxFuture<'a, Result<StepOutcome>> {
        Box::pin(async move {
            let task_goal = context
                .state
                .get::<TaskGoal>()
                .ok_or_else(|| AgentError::explain(ErrorType::Internal, "TaskGoal missing"))?;
            let current_document = context
                .state
                .get::<String>()
                .ok_or_else(|| AgentError::explain(ErrorType::Internal, "Document missing"))?;
            let history = context
                .state
                .get::<FeedbackHistory>()
                .cloned()
                .unwrap_or_default();
            let feedback_list = context
                .state
                .get::<AuditorFeedbackList>()
                .cloned()
                .unwrap_or_default();

            // Combine task goal with auditor feedback
            let mut effective_goal = task_goal.0.clone();
            for feedback in &feedback_list.0 {
                effective_goal =
                    format!("{}\n\nFEEDBACK FROM AUDITOR: {}", effective_goal, feedback);
            }

            let prompt = if history.0.is_empty() {
                tracing::info!("Initial task triggered");
                WriterTemplates::initial_task(&effective_goal, current_document)
            } else {
                tracing::info!("Refinement task triggered based on feedback history");
                // Note: WriterTemplates::refinement_task might need to be updated to handle FeedbackHistory correctly
                // For now we'll assume it takes the history vector
                WriterTemplates::refinement_task(
                    &effective_goal,
                    current_document,
                    &history.0,
                    &Vec::new(), // search results
                )
            };

            tracing::debug!(
                model = self.llm.model_id(),
                prompt_length = prompt.len(),
                "Prepared prompt for Writer agent"
            );

            let agent = self.build_agent(Arc::clone(&context));

            tracing::info!(
                "Executing Writer agent ReAct loop (model: {}). Max autonomous turns: 10.",
                self.llm.model_id()
            );

            let start = Instant::now();
            let text = agent.prompt(&prompt).await.map_err(|e| {
                tracing::error!(
                    error = %e,
                    model = self.llm.model_id(),
                    "Agent autonomous loop failed. Current error: {e}"
                );
                AgentError::explain(
                    ErrorType::Provider,
                    format!(
                        "Agent autonomous loop failed (model: {}): {e}",
                        self.llm.model_id()
                    ),
                )
            })?;
            let duration = start.elapsed().as_millis();
            #[allow(clippy::cast_possible_truncation)]
            let duration = duration as u64;

            tracing::info!(
                response_length = text.len(),
                "Writer agent successfully finished research and drafting"
            );

            let mut next_context = (*context).clone();

            // Robust extraction logic supporting XML-like tags or prefix
            let mut updated = false;
            let trimmed = text.trim();

            if trimmed == "DONE_EDITING" {
                tracing::info!("Writer agent completed via surgical edits");
                updated = true;
            } else if let Some(pos) = text.find("<expanded_document>") {
                let end_pos = text.find("</expanded_document>").unwrap_or(text.len());
                let document_text = &text[pos + "<expanded_document>".len()..end_pos];
                next_context.state.insert(document_text.trim().to_string());
                tracing::info!("Writer agent completed via tagged document block");
                updated = true;
            } else if let Some(document_text) = text.strip_prefix("FULL_DOCUMENT:\n") {
                next_context.state.insert(document_text.to_string());
                tracing::info!("Writer agent completed via full document rewrite (legacy prefix)");
                updated = true;
            } else {
                // Heuristic fallback
                for marker in &["FULL_DOCUMENT:", "DOCUMENT:", "FINAL_OUTPUT:"] {
                    if let Some(pos) = text.find(marker) {
                        let document_text = &text[pos + marker.len()..];
                        next_context.state.insert(document_text.trim().to_string());
                        tracing::info!(marker, "Writer agent completed via heuristic fallback");
                        updated = true;
                        break;
                    }
                }
            }

            if !updated {
                tracing::warn!(
                    response = truncate_chars(&text, 200),
                    "Writer agent returned ambiguous response, document not updated"
                );
            }

            // Estimate usage accurately using centralized utility
            let usage = agent_kernel::TokenEstimator::estimate(&prompt, &text);

            let step = TrajectoryStep::Thought {
                text: format!(
                    "Writer (model: {id}) autonomously researched and finalized the document. Response preview: {preview}...",
                    id = self.llm.model_id(),
                    preview = text.chars().take(100).collect::<String>()
                ),
                usage: Some(usage),
                duration_ms: Some(duration),
            };

            Ok(StepOutcome {
                updated_context: next_context,
                usage: Some(usage),
                trajectory_events: vec![step],
            })
        })
    }
}
