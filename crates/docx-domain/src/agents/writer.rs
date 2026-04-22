use crate::prompts::WriterTemplates;
use agent_kernel::{
    AgentSession, AutonomousAgent, BoxFuture, LanguageModel, Result, SearchProvider, SourceFetcher,
    TrajectoryStep, truncate_chars,
};
use agent_tools::{EditDocumentTool, FetchUrlTool, WebSearchTool};
use rig::completion::Prompt;
use std::sync::Arc;
use std::time::Instant;

pub struct DocumentWriter {
    llm: Arc<dyn LanguageModel>,
    search: Arc<dyn SearchProvider>,
    fetcher: Arc<dyn SourceFetcher>,
}

impl DocumentWriter {
    #[must_use]
    pub fn new(
        llm: Arc<dyn LanguageModel>,
        search: Arc<dyn SearchProvider>,
        fetcher: Arc<dyn SourceFetcher>,
    ) -> Self {
        Self {
            llm,
            search,
            fetcher,
        }
    }

    fn build_agent<'a>(
        &'a self,
        session: &'a AgentSession,
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
                context: session.context.clone(),
                trajectory: session.trajectory.clone(),
            })
            .tool(FetchUrlTool {
                fetcher: self.fetcher.clone(),
                context: session.context.clone(),
                trajectory: session.trajectory.clone(),
            })
            .tool(EditDocumentTool {
                context: session.context.clone(),
                trajectory: session.trajectory.clone(),
            })
            .default_max_turns(10)
            .build()
    }
}

impl AutonomousAgent for DocumentWriter {
    fn role(&self) -> &'static str {
        "Writer"
    }

    fn run<'a>(&'a self, session: &'a AgentSession) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let context = session.context.read().await;

            let prompt = if context.feedback_history.is_empty() {
                tracing::info!("Initial task triggered");
                WriterTemplates::initial_task(&context.task_goal, &context.current_document)
            } else {
                tracing::info!("Refinement task triggered based on feedback history");
                WriterTemplates::refinement_task(
                    &context.task_goal,
                    &context.current_document,
                    &context.feedback_history,
                    &context.search_results,
                )
            };

            drop(context);

            tracing::debug!(
                model = self.llm.model_id(),
                prompt_length = prompt.len(),
                "Prepared prompt for Writer agent"
            );

            let agent = self.build_agent(session);

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
                agent_kernel::Error::explain(
                    agent_kernel::ErrorType::Provider,
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

            let mut context = session.context.write().await;

            // Robust extraction logic supporting XML-like tags or prefix
            let mut updated = false;
            let trimmed = text.trim();

            if trimmed == "DONE_EDITING" {
                tracing::info!("Writer agent completed via surgical edits");
                updated = true;
            } else if let Some(pos) = text.find("<expanded_document>") {
                let end_pos = text.find("</expanded_document>").unwrap_or(text.len());
                let document_text = &text[pos + "<expanded_document>".len()..end_pos];
                context.current_document = document_text.trim().to_string();
                tracing::info!("Writer agent completed via tagged document block");
                updated = true;
            } else if let Some(document_text) = text.strip_prefix("FULL_DOCUMENT:\n") {
                context.current_document = document_text.to_string();
                tracing::info!("Writer agent completed via full document rewrite (legacy prefix)");
                updated = true;
            } else {
                // Heuristic fallback
                for marker in &["FULL_DOCUMENT:", "DOCUMENT:", "FINAL_OUTPUT:"] {
                    if let Some(pos) = text.find(marker) {
                        let document_text = &text[pos + marker.len()..];
                        context.current_document = document_text.trim().to_string();
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

            self.record_telemetry(session, &prompt, &text, duration)
                .await;

            Ok(())
        })
    }
}

impl DocumentWriter {
    async fn record_telemetry(
        &self,
        session: &AgentSession,
        prompt: &str,
        text: &str,
        duration: u64,
    ) {
        let mut telemetry = session.telemetry.lock().await;
        let prompt_tokens = prompt.split_whitespace().count();
        let completion_tokens = text.split_whitespace().count();
        telemetry.add_usage(
            self.llm.model_id(),
            agent_kernel::TokenUsage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            },
        );

        let mut trajectory = session.trajectory.lock().await;
        trajectory.steps.push(TrajectoryStep::Thought {
            text: format!(
                "Writer (model: {id}) autonomously researched and finalized the document. Response preview: {preview}...",
                id = self.llm.model_id(),
                preview = text.chars().take(100).collect::<String>()
            ),
            usage: Some(agent_kernel::TokenUsage {
                prompt_tokens,
                completion_tokens,
                total_tokens: prompt_tokens + completion_tokens,
            }),
            duration_ms: Some(duration),
        });
    }
}
