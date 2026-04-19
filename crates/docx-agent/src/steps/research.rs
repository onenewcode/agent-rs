use std::sync::Arc;
use std::time::Duration;

use agent_core::{
    BoxFuture, ExpansionError, ExpansionRequest, ExpansionResult, ResearchResult, ResearchRuntime,
    Step,
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
        research: Option<ResearchResult>,
    ) -> BoxFuture<'a, Result<(Option<ExpansionResult>, Option<ResearchResult>), ExpansionError>>
    {
        let runtime = Arc::clone(&self.runtime);
        let request_clone = request.clone();
        let timeout_dur = self.timeout;

        Box::pin(async move {
            if let Some(research) = research {
                info!("Skipping research step, reusing existing research");
                return Ok((current_result, Some(research)));
            }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use agent_core::{ParsedDocument, SourceKind};

    struct CountingResearchRuntime {
        calls: Arc<AtomicUsize>,
    }

    impl ResearchRuntime for CountingResearchRuntime {
        fn research(
            &self,
            _request: ExpansionRequest,
        ) -> BoxFuture<'_, Result<ResearchResult, ExpansionError>> {
            let calls = Arc::clone(&self.calls);

            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ResearchResult {
                    search_queries: vec!["query".to_owned()],
                    sources: vec![agent_core::FetchedSource {
                        kind: SourceKind::SearchResult,
                        title: Some("source".to_owned()),
                        url: "https://example.com".to_owned(),
                        summary: None,
                        content: "content".to_owned(),
                    }],
                })
            })
        }
    }

    #[tokio::test]
    async fn research_step_reuses_existing_research_without_calling_runtime() {
        let calls = Arc::new(AtomicUsize::new(0));
        let step = ResearchStep::new(
            Arc::new(CountingResearchRuntime {
                calls: Arc::clone(&calls),
            }),
            1,
        );
        let mut request = ExpansionRequest {
            prompt: "prompt".to_owned(),
            document: ParsedDocument::default(),
            user_urls: Vec::new(),
        };
        let existing = ResearchResult {
            search_queries: vec!["existing".to_owned()],
            sources: vec![agent_core::FetchedSource {
                kind: SourceKind::UserUrl,
                title: Some("kept".to_owned()),
                url: "https://kept.example.com".to_owned(),
                summary: None,
                content: "cached".to_owned(),
            }],
        };

        let (_, research) = step
            .execute(&mut request, None, Some(existing.clone()))
            .await
            .expect("research reuse should succeed");

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(research, Some(existing));
    }
}
