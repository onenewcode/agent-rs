use agent_kernel::{
    AgentSession, AutonomousAgent, BoxFuture, LanguageModel, RunError, SearchProvider,
};
use rig::completion::Prompt;
use std::sync::Arc;

pub struct WriterAgent {
    llm: Arc<dyn LanguageModel>,
    search_provider: Option<Arc<dyn SearchProvider>>,
}

impl WriterAgent {
    pub fn new(
        llm: Arc<dyn LanguageModel>,
        search_provider: Option<Arc<dyn SearchProvider>>,
    ) -> Self {
        Self {
            llm,
            search_provider,
        }
    }
}

impl AutonomousAgent for WriterAgent {
    fn role(&self) -> &'static str {
        "Writer"
    }

    fn run<'a>(&'a self, session: &'a AgentSession) -> BoxFuture<'a, Result<(), RunError>> {
        let llm = self.llm.clone();
        let search = self.search_provider.clone();

        Box::pin(async move {
            let context = session.context.write().await;

            // Build the agent with tools
            let content_wrapper =
                Arc::new(tokio::sync::RwLock::new(context.current_document.clone()));
            let edit_tool = agent_tools::EditDocumentTool {
                current_content: Arc::clone(&content_wrapper),
                trajectory: Arc::clone(&session.trajectory),
            };

            let mut builder = llm.agent_builder().tool(edit_tool);

            if let Some(s) = search {
                builder = builder.tool(agent_tools::WebSearchTool {
                    provider: s,
                    trajectory: Arc::clone(&session.trajectory),
                });
            }

            let agent = builder.default_max_turns(10).build();

            // Construct prompt for the writer, including latest feedback if any
            let latest_feedback = context.feedback_history.last();
            let prompt = if let Some(feedback) = latest_feedback {
                format!(
                    "Goal: {}\n\nCurrent Document: {}\n\nFeedback from Reviewer:\n- Score: {}\n- Suggestions: {:?}\n- Errors: {:?}\n\nPlease improve the document based on the feedback.",
                    context.task_goal,
                    context.current_document,
                    feedback.score,
                    feedback.suggestions,
                    feedback.critical_errors
                )
            } else {
                format!(
                    "Goal: {}\n\nCurrent Document: {}\n\nPlease expand and improve this document autonomously using the tools provided.",
                    context.task_goal, context.current_document
                )
            };

            // Release context lock before long-running agent prompt to allow other agents to read if needed
            // (though in this loop it's sequential, it's good practice)
            drop(context);

            let _response = agent
                .prompt(&prompt)
                .await
                .map_err(|e| RunError::Provider(e.to_string()))?;

            // Update document in context
            let updated_content = content_wrapper.read().await.clone();
            let mut context = session.context.write().await;
            context.current_document = updated_content;

            Ok(())
        })
    }
}
