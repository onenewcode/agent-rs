use std::sync::Arc;
use std::time::Duration;

use agent_core::{
    BoxFuture, ExpansionError, ExpansionRequest, ExpansionResult,
    ResearchResult, ResearchRuntime, Step,
};
use tokio::time::timeout;
use tracing::info;

pub struct ResearchStep {
    runtime: Arc<dyn ResearchRuntime>,
    timeout: Duration,
}

impl ResearchStep {
    #[must_use]
    pub fn new(runtime: Arc<dyn ResearchRuntime>, timeout_secs: u64) -> Self {
        Self {
            runtime,
            timeout: Duration::from_secs(timeout_secs),
        }
    }
}

impl Step for ResearchStep {
    fn name(&self) -> &str {
        "Research"
    }

    fn execute<'a>(
        &self,
        request: &'a mut ExpansionRequest,
        current_result: Option<ExpansionResult>,
        _research: Option<ResearchResult>,
    ) -> BoxFuture<'a, Result<(Option<ExpansionResult>, Option<ResearchResult>), ExpansionError>> {
        let runtime = Arc::clone(&self.runtime);
        let request_clone = request.clone();
        let timeout_dur = self.timeout;

        Box::pin(async move {
            info!("Starting research step");
            let research = timeout(timeout_dur, runtime.research(request_clone))
                .await
                .map_err(|_| {
                    ExpansionError::Timeout(format!(
                        "Research timed out after {}s",
                        timeout_dur.as_secs()
                    ))
                })??;

            Ok((current_result, Some(research)))
        })
    }
}
