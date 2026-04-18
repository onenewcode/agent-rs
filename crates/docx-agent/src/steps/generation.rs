use std::sync::Arc;

use agent_core::{
    BoxFuture, ExpansionError, ExpansionRequest, ExpansionResult, ExpansionRuntime,
    ResearchResult, Step,
};
use tracing::info;

pub struct GenerationStep {
    runtime: Arc<dyn ExpansionRuntime>,
}

impl GenerationStep {
    #[must_use]
    pub fn new(runtime: Arc<dyn ExpansionRuntime>) -> Self {
        Self { runtime }
    }
}

impl Step for GenerationStep {
    fn name(&self) -> &str {
        "Generation"
    }

    fn execute<'a>(
        &self,
        request: &'a mut ExpansionRequest,
        _current_result: Option<ExpansionResult>,
        research: Option<ResearchResult>,
    ) -> BoxFuture<'a, Result<(Option<ExpansionResult>, Option<ResearchResult>), ExpansionError>> {
        let runtime = Arc::clone(&self.runtime);
        let request_clone = request.clone();
        
        Box::pin(async move {
            info!("Starting generation step");
            let research = research.ok_or_else(|| {
                ExpansionError::Internal("Generation step requires research results".to_owned())
            })?;

            let result = runtime.generate(request_clone, research.clone()).await?;
            Ok((Some(result), Some(research)))
        })
    }
}
