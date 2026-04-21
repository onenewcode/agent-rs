use agent_kernel::{
    AgentSession, AutonomousAgent, BoxFuture, LanguageModel, Result, Error, ErrorType, ErrorSource, RetryType, SearchProvider,
};
use std::sync::Arc;

pub struct DocumentWriter {
    llm: Arc<dyn LanguageModel>,
    search: Arc<dyn SearchProvider>,
}

impl DocumentWriter {
    #[must_use]
    pub fn new(llm: Arc<dyn LanguageModel>, search: Arc<dyn SearchProvider>) -> Self {
        Self { llm, search }
    }
}

impl AutonomousAgent for DocumentWriter {
    fn role(&self) -> &'static str {
        "Writer"
    }

    fn run<'a>(&'a self, session: &'a AgentSession) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let context = session.context.read().await;
            let prompt = format!(
                "Goal: {}\n\nCurrent Document: {}\n\nSearch Results: {:?}\n\nFeedback: {:?}\n\nImprove the document.",
                context.task_goal,
                context.current_document,
                context.search_results,
                context.feedback_history
            );
            drop(context);

            let completion = self.llm.complete(&prompt).await.map_err(|e| {
                Box::new(Error::explain(
                    ErrorType::Provider,
                    format!("{} (model: {})", e, self.llm.model_id()),
                )
                .set_source(ErrorSource::Upstream)
                .set_retry(RetryType::Retry))
            })?;

            let mut context = session.context.write().await;
            context.current_document = completion.text;

            let mut telemetry = session.telemetry.lock().await;
            telemetry.add_usage(self.llm.model_id(), completion.usage);

            Ok(())
        })
    }
}
