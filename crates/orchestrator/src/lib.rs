use std::sync::Arc;

use agent_core::{
    BoxFuture, ExpansionError, ExpansionRequest, ExpansionResult, ExpansionRuntime, Pipeline,
    PipelineState, ResearchResult, Step,
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
            let mut current_request = request;
            let mut attempt = 0;
            let mut state = PipelineState::default();

            loop {
                info!(attempt, "Running orchestration pipeline");
                state = pipeline
                    .run_with_state(&mut current_request, state.into_retry_state())
                    .await?;
                let result = state.clone().into_result()?;

                if result.is_qualified
                    || attempt >= max_refinement_attempts
                    || refinement_step.is_none()
                {
                    return Ok(result);
                }

                if let Some(refiner) = &refinement_step {
                    attempt += 1;
                    info!(
                        attempt,
                        score = result.score,
                        "Content unqualified, running refinement step"
                    );
                    let (refined_result, refined_research) = refiner
                        .execute(
                            &mut current_request,
                            Some(result.clone()),
                            state.research().cloned(),
                        )
                        .await?;
                    state = PipelineState::new(
                        refined_result.or(Some(result)),
                        refined_research.or_else(|| state.research().cloned()),
                    );
                } else {
                    return Ok(result);
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use agent_core::{FetchedSource, SourceKind};

    struct CountingResearchStep {
        calls: Arc<AtomicUsize>,
    }

    impl Step for CountingResearchStep {
        fn name(&self) -> &str {
            "Research"
        }

        fn execute<'a>(
            &self,
            _request: &'a mut ExpansionRequest,
            current_result: Option<ExpansionResult>,
            research: Option<ResearchResult>,
        ) -> BoxFuture<'a, Result<(Option<ExpansionResult>, Option<ResearchResult>), ExpansionError>>
        {
            let calls = Arc::clone(&self.calls);

            Box::pin(async move {
                if let Some(research) = research {
                    return Ok((current_result, Some(research)));
                }

                calls.fetch_add(1, Ordering::SeqCst);
                Ok((
                    current_result,
                    Some(ResearchResult {
                        search_queries: vec!["query".to_owned()],
                        sources: vec![FetchedSource {
                            kind: SourceKind::SearchResult,
                            title: Some("source".to_owned()),
                            url: "https://example.com".to_owned(),
                            summary: None,
                            content: "content".to_owned(),
                        }],
                    }),
                ))
            })
        }
    }

    struct CountingGenerationStep {
        calls: Arc<AtomicUsize>,
    }

    impl Step for CountingGenerationStep {
        fn name(&self) -> &str {
            "Generation"
        }

        fn execute<'a>(
            &self,
            request: &'a mut ExpansionRequest,
            _current_result: Option<ExpansionResult>,
            research: Option<ResearchResult>,
        ) -> BoxFuture<'a, Result<(Option<ExpansionResult>, Option<ResearchResult>), ExpansionError>>
        {
            let calls = Arc::clone(&self.calls);
            let prompt = request.prompt.clone();

            Box::pin(async move {
                let research = research.ok_or_else(|| {
                    ExpansionError::Internal("Generation step requires research".to_owned())
                })?;
                calls.fetch_add(1, Ordering::SeqCst);
                Ok((
                    Some(ExpansionResult {
                        content: prompt,
                        search_queries: research.search_queries.clone(),
                        sources: research.sources.clone(),
                        score: 0,
                        is_qualified: false,
                        evaluation_reason: None,
                    }),
                    Some(research),
                ))
            })
        }
    }

    struct CountingEvaluationStep {
        calls: Arc<AtomicUsize>,
    }

    impl Step for CountingEvaluationStep {
        fn name(&self) -> &str {
            "Evaluation"
        }

        fn execute<'a>(
            &self,
            request: &'a mut ExpansionRequest,
            current_result: Option<ExpansionResult>,
            research: Option<ResearchResult>,
        ) -> BoxFuture<'a, Result<(Option<ExpansionResult>, Option<ResearchResult>), ExpansionError>>
        {
            let calls = Arc::clone(&self.calls);
            let prompt = request.prompt.clone();

            Box::pin(async move {
                let mut result = current_result.ok_or_else(|| {
                    ExpansionError::Internal("Evaluation step requires a result".to_owned())
                })?;
                calls.fetch_add(1, Ordering::SeqCst);
                result.is_qualified = prompt.contains("refined");
                result.score = if result.is_qualified { 90 } else { 40 };
                result.evaluation_reason = Some(if result.is_qualified {
                    "qualified".to_owned()
                } else {
                    "needs refinement".to_owned()
                });
                Ok((Some(result), research))
            })
        }
    }

    struct CountingRefinementStep {
        calls: Arc<AtomicUsize>,
    }

    impl Step for CountingRefinementStep {
        fn name(&self) -> &str {
            "Refinement"
        }

        fn execute<'a>(
            &self,
            request: &'a mut ExpansionRequest,
            current_result: Option<ExpansionResult>,
            research: Option<ResearchResult>,
        ) -> BoxFuture<'a, Result<(Option<ExpansionResult>, Option<ResearchResult>), ExpansionError>>
        {
            let calls = Arc::clone(&self.calls);

            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                request.prompt.push_str(" refined");
                Ok((current_result, research))
            })
        }
    }

    #[tokio::test]
    async fn orchestrator_reuses_research_across_refinement_attempts() {
        let research_calls = Arc::new(AtomicUsize::new(0));
        let generation_calls = Arc::new(AtomicUsize::new(0));
        let evaluation_calls = Arc::new(AtomicUsize::new(0));
        let refinement_calls = Arc::new(AtomicUsize::new(0));

        let mut pipeline = Pipeline::new();
        pipeline.add_step(Box::new(CountingResearchStep {
            calls: Arc::clone(&research_calls),
        }));
        pipeline.add_step(Box::new(CountingGenerationStep {
            calls: Arc::clone(&generation_calls),
        }));
        pipeline.add_step(Box::new(CountingEvaluationStep {
            calls: Arc::clone(&evaluation_calls),
        }));

        let orchestrator = AgentOrchestrator::new(
            pipeline,
            Some(Arc::new(CountingRefinementStep {
                calls: Arc::clone(&refinement_calls),
            })),
            1,
        );

        let result = orchestrator
            .expand(ExpansionRequest {
                prompt: "draft".to_owned(),
                document: agent_core::ParsedDocument::default(),
                user_urls: Vec::new(),
            })
            .await
            .expect("orchestrator should succeed");

        assert!(result.is_qualified);
        assert_eq!(research_calls.load(Ordering::SeqCst), 1);
        assert_eq!(generation_calls.load(Ordering::SeqCst), 2);
        assert_eq!(evaluation_calls.load(Ordering::SeqCst), 2);
        assert_eq!(refinement_calls.load(Ordering::SeqCst), 1);
    }
}
