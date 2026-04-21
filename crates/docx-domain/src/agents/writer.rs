use crate::prompts::WriterTemplates;
use agent_kernel::{
    AgentSession, AutonomousAgent, BoxFuture, LanguageModel, Result, SearchProvider, SourceFetcher,
    TrajectoryStep,
};
use agent_tools::{FetchUrlTool, WebSearchTool};
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
}

impl AutonomousAgent for DocumentWriter {
    fn role(&self) -> &'static str {
        "Writer"
    }

    fn run<'a>(&'a self, session: &'a AgentSession) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let context = session.context.read().await;

            let prompt = if let Some(feedback) = context.feedback_history.last() {
                tracing::info!(
                    "Refinement task triggered based on feedback score: {}/10",
                    feedback.score
                );
                WriterTemplates::refinement_task(
                    &context.task_goal,
                    &context.current_document,
                    feedback,
                )
            } else {
                tracing::info!("Initial task triggered");
                WriterTemplates::initial_task(&context.task_goal, &context.current_document)
            };

            drop(context);

            tracing::debug!(
                model = self.llm.model_id(),
                prompt_length = prompt.len(),
                "Prepared prompt for Writer agent"
            );

            // Initialize the Agent with autonomous tools and enough max_turns for complex research
            let agent = self
                .llm
                .agent_builder()
                .preamble(
                    "You are an autonomous document research and writing agent. \
                    You have access to tools to search the web and fetch specific URLs. \
                    \
                    CRITICAL INSTRUCTIONS:\n\
                    1. If you need more information, use `web_search` or `fetch_url` autonomously.\n\
                    2. If a tool returns an error (like 404), do not stop; use other search results or your internal knowledge to proceed.\n\
                    3. Once you have gathered enough information, your FINAL response must be the COMPLETE expanded document text.\n\
                    4. Do not output conversational filler. Output the document directly."
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
                .default_max_turns(10) // Allow up to 10 rounds of tool usage
                .build();

            tracing::info!(
                "Executing Writer agent ReAct loop (model: {}). Max autonomous turns: 10.",
                self.llm.model_id()
            );

            // Run the agent. It will autonomously decide whether to call tools (search/fetch)
            // in a ReAct loop before returning the final expanded document.
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
            context.current_document.clone_from(&text);

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
