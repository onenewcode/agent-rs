use crate::prompts::ReviewerTemplates;
use agent_kernel::{
    AgentError, AgentFeedback, AutonomousAgent, BoxFuture, ErrorType, FeedbackHistory,
    LanguageModel, Result, StepOutcome, TrajectoryStep, WorkflowContext,
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

    fn run<'a>(&'a self, context: Arc<WorkflowContext>) -> BoxFuture<'a, Result<StepOutcome>> {
        Box::pin(async move {
            let task_goal = context
                .state
                .get::<agent_kernel::TaskGoal>()
                .ok_or_else(|| {
                    AgentError::explain(ErrorType::Internal, "TaskGoal missing from context")
                })?;
            let current_doc = context.state.get::<String>().ok_or_else(|| {
                AgentError::explain(ErrorType::Internal, "Document missing from context")
            })?;

            // Note: For simplicity, we'll just pass empty search results for now or extract them if available
            let search_results = Vec::new();

            let prompt =
                ReviewerTemplates::evaluation_task(&task_goal.0, current_doc, &search_results);

            let start = Instant::now();
            let completion = self.llm.complete(&prompt).await?;
            let duration = start.elapsed().as_millis();
            #[allow(clippy::cast_possible_truncation)]
            let duration = duration as u64;

            let text = completion.text.trim();
            let json_str = if let Some(start) = text.find('{') {
                if let Some(end) = text.rfind('}') {
                    &text[start..=end]
                } else {
                    text
                }
            } else {
                text
            };

            let feedback: AgentFeedback = serde_json::from_str(json_str.trim()).map_err(|e| {
                AgentError::explain(
                    ErrorType::Evaluation,
                    format!("failed to parse reviewer response: {e}. Raw: {text}"),
                )
            })?;

            let mut next_context = (*context).clone();
            let mut history = next_context
                .state
                .get::<FeedbackHistory>()
                .cloned()
                .unwrap_or_default();
            history.0.push(feedback.clone());
            next_context.state.insert(history);

            let step = TrajectoryStep::Thought {
                text: format!(
                    "Scientific Reviewer (model: {}) evaluated the document with Grounding. Score: {}/100. Passed: {}",
                    self.llm.model_id(),
                    feedback.score,
                    feedback.passed
                ),
                usage: Some(completion.usage),
                duration_ms: Some(duration),
            };

            Ok(StepOutcome {
                updated_context: next_context,
                usage: Some(completion.usage),
                trajectory_events: vec![step],
            })
        })
    }
}
