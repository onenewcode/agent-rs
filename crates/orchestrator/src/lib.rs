use std::sync::Arc;

use agent_core::{
    BoxFuture, EvaluationRequest, EvaluatorRuntime, ExpansionError, ExpansionRequest,
    ExpansionResult, ExpansionRuntime,
};
use tracing::{info, warn};

pub struct AgentOrchestrator {
    generator: Arc<dyn ExpansionRuntime>,
    evaluator: Arc<dyn EvaluatorRuntime>,
    researcher: Arc<dyn agent_core::ResearchRuntime>,
    min_score: u8,
    max_refinement_attempts: usize,
    refinement_template: String,
}

#[derive(Debug)]
enum OrchestrationState {
    Researching,
    Generating(agent_core::ResearchResult, usize),
    Evaluating(ExpansionResult, agent_core::ResearchResult, usize),
    Refining(ExpansionResult, agent_core::ResearchResult, usize),
    Completed(ExpansionResult),
    Failed(ExpansionError),
}

impl AgentOrchestrator {
    #[must_use]
    pub fn new(
        generator: Arc<dyn ExpansionRuntime>,
        evaluator: Arc<dyn EvaluatorRuntime>,
        researcher: Arc<dyn agent_core::ResearchRuntime>,
        min_score: u8,
        max_refinement_attempts: usize,
        refinement_template: String,
    ) -> Self {
        Self {
            generator,
            evaluator,
            researcher,
            min_score,
            max_refinement_attempts,
            refinement_template,
        }
    }
}

impl ExpansionRuntime for AgentOrchestrator {
    fn generate(
        &self,
        request: ExpansionRequest,
        research: agent_core::ResearchResult,
    ) -> BoxFuture<'_, Result<ExpansionResult, ExpansionError>> {
        self.generator.generate(request, research)
    }

    fn expand(
        &self,
        request: ExpansionRequest,
    ) -> BoxFuture<'_, Result<ExpansionResult, ExpansionError>> {
        let generator = Arc::clone(&self.generator);
        let evaluator = Arc::clone(&self.evaluator);
        let researcher = Arc::clone(&self.researcher);
        let min_score = self.min_score;
        let max_refinement_attempts = self.max_refinement_attempts;
        let refinement_template = self.refinement_template.clone();

        Box::pin(async move {
            let mut state = OrchestrationState::Researching;
            let mut current_request = request.clone();
            let mut last_result: Option<ExpansionResult> = None;

            loop {
                info!(state = ?state, "Orchestrator state transition");
                match state {
                    OrchestrationState::Researching => match researcher.research(current_request.clone()).await {
                        Ok(research) => {
                            state = OrchestrationState::Generating(research, 1);
                        }
                        Err(e) => {
                            state = OrchestrationState::Failed(e);
                        }
                    },
                    OrchestrationState::Generating(research, attempt) => {
                        if attempt > max_refinement_attempts {
                            warn!("Exhausted generation/refinement attempts");
                            return last_result.ok_or_else(|| {
                                ExpansionError::Internal("No result generated after all attempts".to_owned())
                            });
                        }

                        match generator.generate(current_request.clone(), research.clone()).await {
                            Ok(result) => {
                                state = OrchestrationState::Evaluating(result, research, attempt);
                            }
                            Err(e) => {
                                state = OrchestrationState::Failed(e);
                            }
                        }
                    }
                    OrchestrationState::Evaluating(mut result, research, attempt) => {
                        info!("Evaluating generated content");
                        match evaluator
                            .evaluate(EvaluationRequest {
                                prompt: request.prompt.clone(),
                                content: result.content.clone(),
                                sources: research.sources.clone(),
                            })
                            .await
                        {
                            Ok(evaluation) => {
                                result.score = evaluation.score;
                                result.evaluation_reason = Some(evaluation.reason.clone());
                                result.is_qualified = evaluation.score >= min_score;

                                if result.is_qualified {
                                    state = OrchestrationState::Completed(result);
                                } else {
                                    state = OrchestrationState::Refining(result, research, attempt);
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "Evaluation failed, proceeding with current result but marked unqualified");
                                result.is_qualified = false;
                                result.evaluation_reason = Some(format!("Evaluation error: {e}"));
                                state = OrchestrationState::Completed(result);
                            }
                        }
                    }
                    OrchestrationState::Refining(result, research, attempt) => {
                        warn!(
                            score = result.score,
                            min_score,
                            reason = result.evaluation_reason.as_deref().unwrap_or(""),
                            "Content not qualified, preparing refinement"
                        );

                        last_result = Some(result.clone());

                        // Prepare refinement request for next iteration
                        let refinement_prompt = refinement_template
                            .replace("{prompt}", &request.prompt)
                            .replace("{content}", &result.content)
                            .replace("{reason}", result.evaluation_reason.as_deref().unwrap_or(""));

                        current_request.prompt = refinement_prompt;
                        state = OrchestrationState::Generating(research, attempt + 1);
                    }
                    OrchestrationState::Completed(result) => {
                        info!(score = result.score, qualified = result.is_qualified, "Orchestration completed");
                        return Ok(result);
                    }
                    OrchestrationState::Failed(e) => {
                        return Err(e);
                    }
                }
            }
        })
    }
}
