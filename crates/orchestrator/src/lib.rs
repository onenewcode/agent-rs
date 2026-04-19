use std::sync::Arc;

use agent_core::{
    BoxFuture, ExpansionError, ExpansionRequest, ExpansionResult, ExpansionRuntime,
    Pipeline, ResearchResult, Step,
};
use tracing::info;

pub struct AgentOrchestrator {
    pipeline: Pipeline,
    refinement_step: Option<Arc<dyn Step>>,
    max_refinement_attempts: usize,
}

impl AgentOrchestrator {
    #[must_use]
    pub fn new(
        pipeline: Pipeline,
        refinement_step: Option<Arc<dyn Step>>,
        max_refinement_attempts: usize,
    ) -> Self {
        Self {
            pipeline,
            refinement_step,
            max_refinement_attempts,
        }
    }
}

impl ExpansionRuntime for AgentOrchestrator {
    fn generate(
        &self,
        _request: ExpansionRequest,
        _research: ResearchResult,
    ) -> BoxFuture<'_, Result<ExpansionResult, ExpansionError>> {
        Box::pin(async move {
            Err(ExpansionError::Internal(
                "Orchestrator does not support direct generation, use expand()".to_owned(),
            ))
        })
    }

    fn expand(
        &self,
        request: ExpansionRequest,
    ) -> BoxFuture<'_, Result<ExpansionResult, ExpansionError>> {
        let pipeline = &self.pipeline;
        let refinement_step = self.refinement_step.clone();
        let max_refinement_attempts = self.max_refinement_attempts;

        Box::pin(async move {
            let mut current_request = request.clone();
            let mut attempt = 0;

            loop {
                info!(attempt, "Running orchestration pipeline");
                let result = pipeline.run(&mut current_request).await?;

                // Check if qualified or if we can refine
                if result.is_qualified || attempt >= max_refinement_attempts || refinement_step.is_none() {
                    return Ok(result);
                }

                // Refine
                if let Some(refiner) = &refinement_step {
                    attempt += 1;
                    info!(attempt, score = result.score, "Content unqualified, running refinement step");
                    
                    // Note: In this loop, we need to pass the research results between pipeline runs.
                    // However, Pipeline::run currently re-runs research if it's the first step.
                    // This is slightly inefficient but functionally correct if research is idempotent.
                    // To optimize, we'd need to change Pipeline::run to accept/return state.
                    
                    // For now, refinement_step updates current_request.prompt
                    let _ = refiner
                        .execute(&mut current_request, Some(result), None)
                        .await?;
                } else {
                    return Ok(result);
                }
            }
        })
    }
}
