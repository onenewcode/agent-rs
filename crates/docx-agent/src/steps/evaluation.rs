use std::sync::Arc;

use agent_core::{
    BoxFuture, EvaluationRequest, EvaluatorRuntime, ExpansionError, ExpansionRequest,
    ExpansionResult, ResearchResult, Step,
};
use tracing::{info, warn};

pub struct EvaluationStep {
    evaluator: Arc<dyn EvaluatorRuntime>,
    min_score: u8,
}

impl EvaluationStep {
    #[must_use]
    pub fn new(evaluator: Arc<dyn EvaluatorRuntime>, min_score: u8) -> Self {
        Self { evaluator, min_score }
    }
}

impl Step for EvaluationStep {
    fn name(&self) -> &str {
        "Evaluation"
    }

    fn execute<'a>(
        &self,
        request: &'a mut ExpansionRequest,
        current_result: Option<ExpansionResult>,
        research: Option<ResearchResult>,
    ) -> BoxFuture<'a, Result<(Option<ExpansionResult>, Option<ResearchResult>), ExpansionError>> {
        let evaluator = Arc::clone(&self.evaluator);
        let min_score = self.min_score;
        let prompt = request.prompt.clone();
        
        Box::pin(async move {
            info!("Starting evaluation step");
            let mut result = current_result.ok_or_else(|| {
                ExpansionError::Internal("Evaluation step requires a prior result".to_owned())
            })?;
            let research = research.ok_or_else(|| {
                ExpansionError::Internal("Evaluation step requires research results".to_owned())
            })?;

            match evaluator.evaluate(EvaluationRequest {
                prompt,
                content: result.content.clone(),
                sources: research.sources.clone(),
            }).await {
                Ok(evaluation) => {
                    result.score = evaluation.score;
                    result.evaluation_reason = Some(evaluation.reason);
                    result.is_qualified = evaluation.score >= min_score;
                }
                Err(e) => {
                    warn!(error = %e, "Evaluation failed, marking as unqualified");
                    result.is_qualified = false;
                    result.evaluation_reason = Some(format!("Evaluation error: {e}"));
                }
            }

            Ok((Some(result), Some(research)))
        })
    }
}
