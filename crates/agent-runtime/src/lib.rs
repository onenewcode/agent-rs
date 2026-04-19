#![allow(clippy::missing_errors_doc)]

use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};

use agent_kernel::{
    AttemptReport, Draft, Evaluation, Evaluator, Generator, Plan, Planner, ResearchArtifacts,
    Researcher, RunError, RunReport, SearchMode, SearchProvider, SourceFetcher, StageEvent, Task,
};
use tokio::{sync::Semaphore, time::timeout};
use tracing::warn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSettings {
    pub min_score: u8,
    pub global_timeout_secs: u64,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            min_score: 80,
            global_timeout_secs: 180,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchSettings {
    pub search_max_results: usize,
    pub fetch_concurrency_limit: usize,
}

impl Default for ResearchSettings {
    fn default() -> Self {
        Self {
            search_max_results: 5,
            fetch_concurrency_limit: 5,
        }
    }
}

pub struct DefaultResearcher {
    fetcher: Arc<dyn SourceFetcher>,
    search_provider: Option<Arc<dyn SearchProvider>>,
    settings: ResearchSettings,
}

impl DefaultResearcher {
    #[must_use]
    pub fn new(
        fetcher: Arc<dyn SourceFetcher>,
        search_provider: Option<Arc<dyn SearchProvider>>,
        settings: ResearchSettings,
    ) -> Self {
        Self {
            fetcher,
            search_provider,
            settings,
        }
    }
}

impl Researcher for DefaultResearcher {
    fn research(
        &self,
        task: Task,
        plan: Plan,
    ) -> agent_kernel::BoxFuture<'_, Result<ResearchArtifacts, RunError>> {
        let fetcher = Arc::clone(&self.fetcher);
        let search_provider = self.search_provider.clone();
        let settings = self.settings.clone();

        Box::pin(async move {
            let user_sources =
                collect_user_sources(fetcher, &task.user_urls, settings.fetch_concurrency_limit)
                    .await;
            let search_sources =
                collect_search_sources(search_provider, &plan, settings.search_max_results).await?;

            let mut queries = plan.search_queries.clone();
            let mut sources = user_sources;
            sources.extend(search_sources);
            deduplicate_sources(&mut sources);
            queries.dedup();

            Ok(ResearchArtifacts { queries, sources })
        })
    }
}

async fn collect_user_sources(
    fetcher: Arc<dyn SourceFetcher>,
    urls: &[String],
    concurrency_limit: usize,
) -> Vec<agent_kernel::SourceMaterial> {
    if urls.is_empty() {
        return Vec::new();
    }

    let semaphore = Arc::new(Semaphore::new(concurrency_limit.max(1)));
    let mut set = tokio::task::JoinSet::new();

    for url in urls {
        let fetcher = Arc::clone(&fetcher);
        let url = url.clone();
        let semaphore = Arc::clone(&semaphore);

        set.spawn(async move {
            let _permit = semaphore
                .acquire_owned()
                .await
                .map_err(|error| RunError::Internal(error.to_string()))?;
            fetcher.fetch(&url).await
        });
    }

    let mut sources = Vec::with_capacity(urls.len());
    while let Some(result) = set.join_next().await {
        match result {
            Ok(Ok(source)) => sources.push(source),
            Ok(Err(error)) => warn!(error = %error, "failed to fetch user URL, skipping"),
            Err(error) => warn!(error = %error, "user URL task failed"),
        }
    }

    sources
}

async fn collect_search_sources(
    search_provider: Option<Arc<dyn SearchProvider>>,
    plan: &Plan,
    max_results: usize,
) -> Result<Vec<agent_kernel::SourceMaterial>, RunError> {
    match plan.search_mode {
        SearchMode::Disabled => Ok(Vec::new()),
        SearchMode::Auto | SearchMode::Required => {
            if plan.search_queries.is_empty() {
                return Ok(Vec::new());
            }

            let Some(search_provider) = search_provider else {
                if plan.search_mode == SearchMode::Required {
                    return Err(RunError::Internal(
                        "search was required by plan but no search provider is configured"
                            .to_owned(),
                    ));
                }
                return Ok(Vec::new());
            };

            let mut sources = Vec::new();
            for query in &plan.search_queries {
                match search_provider.search(query, max_results).await {
                    Ok(mut results) => sources.append(&mut results),
                    Err(error) => {
                        if plan.search_mode == SearchMode::Required {
                            return Err(error);
                        }
                        warn!(query, error = %error, "search failed for optional query");
                    }
                }
            }

            Ok(sources)
        }
    }
}

fn deduplicate_sources(sources: &mut Vec<agent_kernel::SourceMaterial>) {
    let mut seen = HashSet::new();
    sources.retain(|source| seen.insert(source.url.clone()));
}

pub struct AgentRuntime {
    planner: Arc<dyn Planner>,
    researcher: Arc<dyn Researcher>,
    generator: Arc<dyn Generator>,
    evaluator: Arc<dyn Evaluator>,
    refiner: Arc<dyn agent_kernel::Refiner>,
    settings: RuntimeSettings,
}

impl AgentRuntime {
    #[must_use]
    pub fn builder(settings: RuntimeSettings) -> RuntimeBuilder {
        RuntimeBuilder::new(settings)
    }

    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn run(&self, task: Task) -> agent_kernel::BoxFuture<'_, Result<RunReport, RunError>> {
        let planner = Arc::clone(&self.planner);
        let researcher = Arc::clone(&self.researcher);
        let generator = Arc::clone(&self.generator);
        let evaluator = Arc::clone(&self.evaluator);
        let refiner = Arc::clone(&self.refiner);
        let settings = self.settings.clone();

        Box::pin(async move {
            timeout(
                Duration::from_secs(settings.global_timeout_secs),
                async move {
                    let run_started = Instant::now();
                    let mut stage_events = Vec::new();

                    let plan = record_stage(&mut stage_events, "plan", 0, || async {
                        planner.plan(task.clone()).await
                    })
                    .await?;

                    let research = record_stage(&mut stage_events, "research", 0, || async {
                        if task.constraints.disable_research {
                            Ok(ResearchArtifacts::default())
                        } else {
                            researcher.research(task.clone(), plan.clone()).await
                        }
                    })
                    .await?;

                    let mut attempts = Vec::new();
                    let mut current_draft =
                        record_stage(&mut stage_events, "generate", 0, || async {
                            generator
                                .generate(task.clone(), plan.clone(), research.clone())
                                .await
                        })
                        .await?;

                    let evaluation = evaluate_with_fallback(
                        &mut stage_events,
                        &*evaluator,
                        task.clone(),
                        plan.clone(),
                        research.clone(),
                        current_draft.clone(),
                        settings.min_score,
                        0,
                    )
                    .await;
                    attempts.push(AttemptReport {
                        attempt: 0,
                        draft: current_draft.clone(),
                        evaluation: evaluation.clone(),
                    });

                    let mut final_evaluation = evaluation;
                    let max_rounds = plan.max_refinement_rounds;

                    for attempt in 1..=max_rounds {
                        if final_evaluation.qualified {
                            break;
                        }

                        current_draft =
                            record_stage(&mut stage_events, "refine", attempt, || async {
                                refiner
                                    .refine(
                                        task.clone(),
                                        plan.clone(),
                                        research.clone(),
                                        current_draft.clone(),
                                        final_evaluation.clone(),
                                    )
                                    .await
                            })
                            .await?;

                        final_evaluation = evaluate_with_fallback(
                            &mut stage_events,
                            &*evaluator,
                            task.clone(),
                            plan.clone(),
                            research.clone(),
                            current_draft.clone(),
                            settings.min_score,
                            attempt,
                        )
                        .await;
                        attempts.push(AttemptReport {
                            attempt,
                            draft: current_draft.clone(),
                            evaluation: final_evaluation.clone(),
                        });
                    }

                    let final_output = attempts
                        .last()
                        .map(|attempt| attempt.draft.content.clone())
                        .unwrap_or_default();
                    let final_reason = attempts
                        .last()
                        .map(|attempt| attempt.evaluation.reason.clone());

                    Ok(RunReport {
                        plan,
                        research,
                        final_score: final_evaluation.score,
                        qualified: final_evaluation.qualified,
                        final_output,
                        final_reason,
                        attempts,
                        stage_events,
                        total_duration_ms: run_started.elapsed().as_millis(),
                    })
                },
            )
            .await
            .map_err(|_| {
                RunError::Timeout(format!(
                    "workflow timed out after {}s",
                    settings.global_timeout_secs
                ))
            })?
        })
    }
}

#[allow(clippy::too_many_arguments)]
async fn evaluate_with_fallback(
    stage_events: &mut Vec<StageEvent>,
    evaluator: &dyn Evaluator,
    task: Task,
    plan: Plan,
    research: ResearchArtifacts,
    draft: Draft,
    min_score: u8,
    attempt: usize,
) -> Evaluation {
    match record_stage(stage_events, "evaluate", attempt, || async {
        evaluator
            .evaluate(task, plan, research, draft.clone())
            .await
    })
    .await
    {
        Ok(mut evaluation) => {
            evaluation.qualified = evaluation.score >= min_score;
            evaluation
        }
        Err(error) => {
            warn!(attempt, error = %error, "evaluation failed, marking draft as unqualified");
            Evaluation {
                score: 0,
                reason: format!("Evaluation failed: {error}"),
                qualified: false,
            }
        }
    }
}

async fn record_stage<T, F, Fut>(
    stage_events: &mut Vec<StageEvent>,
    stage: &str,
    attempt: usize,
    action: F,
) -> Result<T, RunError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, RunError>>,
{
    let started = Instant::now();
    let result = action().await;
    let duration_ms = started.elapsed().as_millis();
    let outcome = result
        .as_ref()
        .map_or_else(std::string::ToString::to_string, |_| "ok".to_owned());
    stage_events.push(StageEvent {
        stage: stage.to_owned(),
        attempt,
        duration_ms,
        outcome,
    });
    result
}

pub struct RuntimeBuilder {
    planner: Option<Arc<dyn Planner>>,
    researcher: Option<Arc<dyn Researcher>>,
    generator: Option<Arc<dyn Generator>>,
    evaluator: Option<Arc<dyn Evaluator>>,
    refiner: Option<Arc<dyn agent_kernel::Refiner>>,
    settings: RuntimeSettings,
}

impl RuntimeBuilder {
    #[must_use]
    pub fn new(settings: RuntimeSettings) -> Self {
        Self {
            planner: None,
            researcher: None,
            generator: None,
            evaluator: None,
            refiner: None,
            settings,
        }
    }

    #[must_use]
    pub fn with_planner(mut self, planner: Arc<dyn Planner>) -> Self {
        self.planner = Some(planner);
        self
    }

    #[must_use]
    pub fn with_researcher(mut self, researcher: Arc<dyn Researcher>) -> Self {
        self.researcher = Some(researcher);
        self
    }

    #[must_use]
    pub fn with_generator(mut self, generator: Arc<dyn Generator>) -> Self {
        self.generator = Some(generator);
        self
    }

    #[must_use]
    pub fn with_evaluator(mut self, evaluator: Arc<dyn Evaluator>) -> Self {
        self.evaluator = Some(evaluator);
        self
    }

    #[must_use]
    pub fn with_refiner(mut self, refiner: Arc<dyn agent_kernel::Refiner>) -> Self {
        self.refiner = Some(refiner);
        self
    }

    pub fn build(self) -> Result<AgentRuntime, RunError> {
        Ok(AgentRuntime {
            planner: self.planner.ok_or_else(|| {
                RunError::Internal("runtime planner is not configured".to_owned())
            })?,
            researcher: self.researcher.ok_or_else(|| {
                RunError::Internal("runtime researcher is not configured".to_owned())
            })?,
            generator: self.generator.ok_or_else(|| {
                RunError::Internal("runtime generator is not configured".to_owned())
            })?,
            evaluator: self.evaluator.ok_or_else(|| {
                RunError::Internal("runtime evaluator is not configured".to_owned())
            })?,
            refiner: self.refiner.ok_or_else(|| {
                RunError::Internal("runtime refiner is not configured".to_owned())
            })?,
            settings: self.settings,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use agent_kernel::{
        Document, Draft, Evaluation, Generator, Plan, Planner, Refiner, ResearchArtifacts,
        Researcher, RunConstraints, RunError, SearchMode, Task,
    };

    use super::{AgentRuntime, RuntimeSettings};

    struct CountingPlanner;

    impl Planner for CountingPlanner {
        fn plan(&self, _task: Task) -> agent_kernel::BoxFuture<'_, Result<Plan, RunError>> {
            Box::pin(async {
                Ok(Plan {
                    objective: "扩写文档".to_owned(),
                    search_mode: SearchMode::Disabled,
                    search_queries: Vec::new(),
                    evaluation_focus: "准确性".to_owned(),
                    max_refinement_rounds: 1,
                })
            })
        }
    }

    struct StaticResearcher;

    impl Researcher for StaticResearcher {
        fn research(
            &self,
            _task: Task,
            _plan: Plan,
        ) -> agent_kernel::BoxFuture<'_, Result<ResearchArtifacts, RunError>> {
            Box::pin(async { Ok(ResearchArtifacts::default()) })
        }
    }

    struct CountingGenerator {
        calls: Arc<AtomicUsize>,
    }

    impl Generator for CountingGenerator {
        fn generate(
            &self,
            task: Task,
            _plan: Plan,
            _research: ResearchArtifacts,
        ) -> agent_kernel::BoxFuture<'_, Result<Draft, RunError>> {
            let calls = Arc::clone(&self.calls);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Draft {
                    content: task.prompt,
                    outline: None,
                })
            })
        }
    }

    struct CountingEvaluator {
        calls: Arc<AtomicUsize>,
    }

    impl agent_kernel::Evaluator for CountingEvaluator {
        fn evaluate(
            &self,
            _task: Task,
            _plan: Plan,
            _research: ResearchArtifacts,
            draft: Draft,
        ) -> agent_kernel::BoxFuture<'_, Result<Evaluation, RunError>> {
            let calls = Arc::clone(&self.calls);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                let qualified = draft.content.contains("refined");
                Ok(Evaluation {
                    score: if qualified { 90 } else { 40 },
                    reason: if qualified {
                        "qualified".to_owned()
                    } else {
                        "needs refinement".to_owned()
                    },
                    qualified,
                })
            })
        }
    }

    struct CountingRefiner {
        calls: Arc<AtomicUsize>,
    }

    impl Refiner for CountingRefiner {
        fn refine(
            &self,
            _task: Task,
            _plan: Plan,
            _research: ResearchArtifacts,
            mut draft: Draft,
            _evaluation: Evaluation,
        ) -> agent_kernel::BoxFuture<'_, Result<Draft, RunError>> {
            let calls = Arc::clone(&self.calls);
            Box::pin(async move {
                calls.fetch_add(1, Ordering::SeqCst);
                draft.content.push_str(" refined");
                Ok(draft)
            })
        }
    }

    #[tokio::test]
    async fn runtime_runs_refinement_loop_until_qualified() {
        let generation_calls = Arc::new(AtomicUsize::new(0));
        let evaluation_calls = Arc::new(AtomicUsize::new(0));
        let refinement_calls = Arc::new(AtomicUsize::new(0));

        let runtime = AgentRuntime::builder(RuntimeSettings::default())
            .with_planner(Arc::new(CountingPlanner))
            .with_researcher(Arc::new(StaticResearcher))
            .with_generator(Arc::new(CountingGenerator {
                calls: Arc::clone(&generation_calls),
            }))
            .with_evaluator(Arc::new(CountingEvaluator {
                calls: Arc::clone(&evaluation_calls),
            }))
            .with_refiner(Arc::new(CountingRefiner {
                calls: Arc::clone(&refinement_calls),
            }))
            .build()
            .expect("runtime should build");

        let report = runtime
            .run(Task {
                prompt: "draft".to_owned(),
                document: Document::default(),
                user_urls: Vec::new(),
                constraints: RunConstraints::default(),
            })
            .await
            .expect("runtime should succeed");

        assert!(report.qualified);
        assert_eq!(generation_calls.load(Ordering::SeqCst), 1);
        assert_eq!(evaluation_calls.load(Ordering::SeqCst), 2);
        assert_eq!(refinement_calls.load(Ordering::SeqCst), 1);
    }
}
