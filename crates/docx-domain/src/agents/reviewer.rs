use crate::prompts::ReviewerTemplates;
use agent_kernel::{
    AgentFeedback, AgentSession, AutonomousAgent, BoxFuture, Error, ErrorType, LanguageModel,
    Result, TrajectoryStep,
};
use std::sync::Arc;
use std::time::Instant;

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
            let prompt =
                ReviewerTemplates::evaluation_task(&context.task_goal, &context.current_document);
            drop(context);

            let start = Instant::now();
            let completion = self.llm.complete(&prompt).await?;
            let duration = start.elapsed().as_millis();
            #[allow(clippy::cast_possible_truncation)]
            let duration = duration as u64;

            let feedback: AgentFeedback = serde_json::from_str(&completion.text).map_err(|e| {
                Box::new(Error::explain(
                    ErrorType::Evaluation,
                    format!("failed to parse reviewer response: {e}"),
                ))
            })?;

            let mut context = session.context.write().await;
            context.feedback_history.push(feedback.clone());

            let mut telemetry = session.telemetry.lock().await;
            telemetry.add_usage(self.llm.model_id(), completion.usage);

            let mut trajectory = session.trajectory.lock().await;
            trajectory.steps.push(TrajectoryStep::Thought {
                text: format!(
                    "Reviewer (model: {}) evaluated the document. Score: {}/10. Passed: {}",
                    self.llm.model_id(),
                    feedback.score,
                    feedback.passed
                ),
                usage: Some(completion.usage),
                duration_ms: Some(duration),
            });

            Ok(())
        })
    }
}
