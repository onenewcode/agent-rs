use agent_kernel::{
    AgentFeedback, AgentSession, AutonomousAgent, BoxFuture, LanguageModel, Result, Error, ErrorType, ErrorSource, RetryType,
};
use std::sync::Arc;

pub struct DocumentReviewer {
    llm: Arc<dyn LanguageModel>,
}

impl DocumentReviewer {
    #[must_use]
    pub fn new(llm: Arc<dyn LanguageModel>) -> Self {
        Self { llm }
    }
}

impl AutonomousAgent for DocumentReviewer {
    fn role(&self) -> &'static str {
        "Reviewer"
    }

    fn run<'a>(&'a self, session: &'a AgentSession) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let context = session.context.read().await;
            let prompt = format!(
                "Review the following document based on the goal: {}\n\nDocument: {}\n\nProvide feedback in JSON format: {{\"score\": 0-10, \"passed\": bool, \"suggestions\": [], \"critical_errors\": []}}",
                context.task_goal,
                context.current_document
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

            let feedback: AgentFeedback = serde_json::from_str(&completion.text).map_err(|e| {
                Box::new(Error::explain(
                    ErrorType::Evaluation,
                    format!("failed to parse reviewer response: {e}"),
                ))
            })?;

            let mut context = session.context.write().await;
            context.feedback_history.push(feedback);

            let mut telemetry = session.telemetry.lock().await;
            telemetry.add_usage(self.llm.model_id(), completion.usage);

            Ok(())
        })
    }
}
