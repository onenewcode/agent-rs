use agent_kernel::{
    AgentFeedback, AgentSession, AutonomousAgent, BoxFuture, LanguageModel, RunError,
};
use serde::Deserialize;
use std::sync::Arc;

pub struct ReviewerAgent {
    llm: Arc<dyn LanguageModel>,
    min_score: u8,
}

impl ReviewerAgent {
    pub fn new(llm: Arc<dyn LanguageModel>, min_score: u8) -> Self {
        Self { llm, min_score }
    }
}

#[derive(Deserialize)]
struct EvaluationPayload {
    score: u8,
    passed: bool,
    suggestions: Vec<String>,
    critical_errors: Vec<String>,
}

impl AutonomousAgent for ReviewerAgent {
    fn role(&self) -> &'static str {
        "Reviewer"
    }

    fn run<'a>(&'a self, session: &'a AgentSession) -> BoxFuture<'a, Result<(), RunError>> {
        let llm = self.llm.clone();
        let min_score = self.min_score;

        Box::pin(async move {
            let context = session.context.read().await;

            let prompt = crate::prompts::ReviewerTemplates::evaluation_task(
                &context.task_goal,
                &context.current_document,
            );

            tracing::info!(role = "Reviewer", model = self.llm.model_id(), "Starting evaluation");
            let completion = llm.complete(&prompt).await.map_err(|e| {
                RunError::Provider(format!("{} (model: {})", e, self.llm.model_id()))
            })?;

            let payload: EvaluationPayload =
                parse_json_response(&completion.text).map_err(|e| {
                    RunError::Evaluation(format!("failed to parse reviewer response: {e}"))
                })?;

            let feedback = AgentFeedback {
                score: payload.score,
                passed: payload.passed && payload.score >= min_score,
                suggestions: payload.suggestions,
                critical_errors: payload.critical_errors,
            };

            drop(context);
            let mut context = session.context.write().await;
            context.feedback_history.push(feedback);

            Ok(())
        })
    }
}

fn parse_json_response(text: &str) -> Result<EvaluationPayload, serde_json::Error> {
    let trimmed = text.trim();
    let json = if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        &trimmed[start..=end]
    } else {
        trimmed
    };
    serde_json::from_str(json)
}
