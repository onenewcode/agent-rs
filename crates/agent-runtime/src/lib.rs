#![allow(clippy::missing_errors_doc)]

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use agent_kernel::{
    CapabilityRegistry, EventStatus, RunError, RunEvent, RunReport, RunRequest, StepTransition,
    Workflow, WorkflowContext, WorkflowDefinition,
};
use tokio::time::timeout;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorSettings {
    pub default_timeout_secs: u64,
    pub capture_artifacts: bool,
}

impl Default for ExecutorSettings {
    fn default() -> Self {
        Self {
            default_timeout_secs: 180,
            capture_artifacts: true,
        }
    }
}

pub struct WorkflowExecutor {
    services: Arc<dyn CapabilityRegistry>,
    workflows: BTreeMap<String, Arc<dyn Workflow>>,
    settings: ExecutorSettings,
}

impl WorkflowExecutor {
    #[must_use]
    pub fn builder(
        services: Arc<dyn CapabilityRegistry>,
        settings: ExecutorSettings,
    ) -> WorkflowExecutorBuilder {
        WorkflowExecutorBuilder::new(services, settings)
    }

    #[must_use]
    pub fn run(&self, request: RunRequest) -> agent_kernel::BoxFuture<'_, Result<RunReport, RunError>> {
        let Some(workflow) = self.workflows.get(&request.workflow).cloned() else {
            let workflow_id = request.workflow.clone();
            return Box::pin(async move {
                Err(RunError::Workflow(format!(
                    "workflow `{workflow_id}` is not registered"
                )))
            });
        };

        let services = Arc::clone(&self.services);
        let settings = self.settings.clone();

        Box::pin(async move {
            let definition = workflow.build(&request)?;
            let timeout_secs = if request.options.global_timeout_secs == 0 {
                settings.default_timeout_secs
            } else {
                request.options.global_timeout_secs
            };

            execute_workflow(services, definition, request, settings.capture_artifacts, timeout_secs).await
        })
    }
}

pub struct WorkflowExecutorBuilder {
    services: Arc<dyn CapabilityRegistry>,
    workflows: BTreeMap<String, Arc<dyn Workflow>>,
    settings: ExecutorSettings,
}

impl WorkflowExecutorBuilder {
    #[must_use]
    pub fn new(services: Arc<dyn CapabilityRegistry>, settings: ExecutorSettings) -> Self {
        Self {
            services,
            workflows: BTreeMap::new(),
            settings,
        }
    }

    #[must_use]
    pub fn register_workflow(mut self, workflow: Arc<dyn Workflow>) -> Self {
        self.workflows.insert(workflow.id().to_owned(), workflow);
        self
    }

    #[must_use]
    pub fn build(self) -> WorkflowExecutor {
        WorkflowExecutor {
            services: self.services,
            workflows: self.workflows,
            settings: self.settings,
        }
    }
}

#[allow(clippy::too_many_lines)]
async fn execute_workflow(
    services: Arc<dyn CapabilityRegistry>,
    definition: WorkflowDefinition,
    request: RunRequest,
    capture_artifacts: bool,
    timeout_secs: u64,
) -> Result<RunReport, RunError> {
    let run_started = Instant::now();
    let run_id = generate_run_id(definition.workflow_id);
    let mut context = WorkflowContext::new(
        run_id.clone(),
        definition.workflow_id.to_owned(),
        request.input,
        services.clone(),
    );
    let mut events = Vec::new();
    let mut current_step_id = definition.initial_step;
    let mut output_artifact = None;
    let mut qualified = false;
    let capture_artifacts = capture_artifacts && request.options.capture_artifacts;

    // Execute with timeout but ensure cleanup and report generation
    let result: Result<(), RunError> = match timeout(Duration::from_secs(timeout_secs), async {
        loop {
            let step_config = definition.steps.get(current_step_id).ok_or_else(|| {
                RunError::Workflow(format!(
                    "workflow `{}` tried to execute undefined step `{current_step_id}`",
                    definition.workflow_id
                ))
            })?;

            let mut attempts = 0;
            let max_attempts = step_config.retry_policy.as_ref().map_or(1, |p| p.max_attempts);

            let execution_result = loop {
                events.push(RunEvent {
                    step_id: current_step_id.to_owned(),
                    status: EventStatus::Started,
                    duration_ms: 0,
                    message: if attempts > 0 {
                        Some(format!("Retry attempt {attempts}"))
                    } else {
                        None
                    },
                });

                let step_started = Instant::now();
                match step_config.step.execute(&mut context).await {
                    Ok(transition) => {
                        let duration_ms = step_started.elapsed().as_millis();
                        events.push(RunEvent {
                            step_id: current_step_id.to_owned(),
                            status: EventStatus::Succeeded,
                            duration_ms,
                            message: None,
                        });
                        break Ok(transition);
                    }
                    Err(error) => {
                        attempts += 1;
                        let duration_ms = step_started.elapsed().as_millis();
                        events.push(RunEvent {
                            step_id: current_step_id.to_owned(),
                            status: EventStatus::Failed,
                            duration_ms,
                            message: Some(error.to_string()),
                        });

                        if attempts < max_attempts {
                            let delay = step_config.retry_policy.as_ref().unwrap().base_delay_ms * (2u64.pow(u32::try_from(attempts - 1).unwrap_or(0)));
                            tracing::warn!(
                                workflow = definition.workflow_id,
                                step = current_step_id,
                                attempt = attempts,
                                delay_ms = delay,
                                duration_ms = duration_ms,
                                error = %sanitize_error_msg(&error),
                                "Step failed; retrying with backoff"
                            );
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                        } else {
                            tracing::error!(
                                workflow = definition.workflow_id,
                                step = current_step_id,
                                attempts = attempts,
                                duration_ms = duration_ms,
                                error = %sanitize_error_msg(&error),
                                "Step failed and exhausted all retries"
                            );
                            break Err(error);
                        }
                    }
                }
            };

            match execution_result {
                Ok(transition) => match transition {
                    StepTransition::Next(next_step) => {
                        current_step_id = next_step;
                    }
                    StepTransition::Complete {
                        output_artifact: requested_output,
                        qualified: completed_qualified,
                    } => {
                        qualified = completed_qualified;
                        output_artifact = requested_output;
                        return Ok(());
                    }
                },
                Err(error) => {
                    if let Some(fallback) = step_config.fallback_step {
                        tracing::info!(
                            workflow = definition.workflow_id,
                            failed_step = current_step_id,
                            fallback_step = fallback,
                            "Transitioning to fallback step after retry exhaustion"
                        );
                        current_step_id = fallback;
                    } else {
                        return Err(error);
                    }
                }
            }
        }
    }).await {
        Ok(res) => res,
        Err(_) => Err(RunError::Timeout(format!("workflow timed out after {timeout_secs}s"))),
    };

    // Create report even on failure to ensure persistence of events and artifacts
    let report = RunReport {
        run_id,
        workflow: definition.workflow_id.to_owned(),
        qualified,
        output_artifact,
        artifacts: if capture_artifacts {
            context.artifacts.clone()
        } else {
            Vec::new()
        },
        events,
        telemetry: context.state::<agent_kernel::Telemetry>().cloned().unwrap_or_default(),
        trajectory: context.state::<agent_kernel::AgentTrajectory>().cloned().unwrap_or_default(),
        total_duration_ms: run_started.elapsed().as_millis(),
    };

    // Always attempt to persist the report
    if let Some(store) = services.artifact_store() {
        if let Err(err) = store.persist(&report).await {
            tracing::error!(error = %err, "Failed to persist run report");
        }
    }

    match result {
        Ok(()) => Ok(report),
        Err(err) => Err(err),
    }
}

fn generate_run_id(workflow: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{workflow}-{millis}")
}

fn sanitize_error_msg(err: &impl std::fmt::Display) -> String {
    let s = err.to_string();
    let trimmed = s.trim();

    // Compress multiple consecutive newlines (common in 524 errors)
    if trimmed.contains("\n\n\n") {
        let mut lines = Vec::new();
        for line in trimmed.lines() {
            let line_trimmed = line.trim();
            if !line_trimmed.is_empty() {
                lines.push(line_trimmed);
            }
        }
        return lines.join(" | "); // Join with pipe for compactness
    }

    trimmed.to_owned()
}


#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{Arc, Mutex},
    };

    use agent_kernel::{
        ArtifactStore, CapabilityRegistry, DocumentParser, LanguageModel, QualityGate,
        RunOptions, SearchProvider, SourceFetcher, Workflow, WorkflowDefinition,
        WorkflowStep, ArtifactEnvelope,
    };

    use super::{ExecutorSettings, WorkflowExecutor};

    struct EmptyServices;

    impl CapabilityRegistry for EmptyServices {
        fn llm(&self, _name: &str) -> Result<Arc<dyn LanguageModel>, agent_kernel::RunError> {
            Ok(Arc::new(MockLlm))
        }

        fn source_fetcher(
            &self,
        ) -> Result<Arc<dyn SourceFetcher>, agent_kernel::RunError> {
            Err(agent_kernel::RunError::Internal("no fetcher configured".to_owned()))
        }

        fn search_provider(&self) -> Option<Arc<dyn SearchProvider>> {
            None
        }

        fn artifact_store(&self) -> Option<Arc<dyn ArtifactStore>> {
            None
        }
    }

    struct MockLlm;

    impl LanguageModel for MockLlm {
        fn complete<'a>(&'a self, _context: &'a mut agent_kernel::WorkflowContext, _prompt: &str) -> agent_kernel::BoxFuture<'a, Result<String, agent_kernel::RunError>> {
            Box::pin(async { Ok("mock response".to_string()) })
        }

        fn agent_builder(&self) -> rig::agent::AgentBuilder<rig::providers::openrouter::completion::CompletionModel> {
            panic!("agent_builder not implemented for MockLlm");
        }
    }

    struct GateWorkflow;

    impl Workflow for GateWorkflow {
        fn id(&self) -> &'static str {
            "tests.gate"
        }

        fn build(
            &self,
            _request: &agent_kernel::RunRequest,
        ) -> Result<WorkflowDefinition, agent_kernel::RunError> {
            Ok(WorkflowDefinition::new(
                self.id(),
                "generate",
                vec![
                    agent_kernel::StepConfig::new(Arc::new(GenerateStep)),
                    agent_kernel::StepConfig::new(Arc::new(EvaluateStep)),
                    agent_kernel::StepConfig::new(Arc::new(FinalizeStep)),
                    agent_kernel::StepConfig::new(Arc::new(RefineStep)),
                ],
            ))
        }
    }

    struct AttemptCount(usize);

    struct GenerateStep;

    impl WorkflowStep for GenerateStep {
        fn id(&self) -> &'static str {
            "generate"
        }

        fn execute<'a>(
            &self,
            context: &'a mut agent_kernel::WorkflowContext,
        ) -> agent_kernel::BoxFuture<'a, Result<agent_kernel::StepTransition, agent_kernel::RunError>> {
            Box::pin(async move {
                context.insert_state(AttemptCount(0));
                context.insert_state("draft-0".to_string());
                Ok(agent_kernel::StepTransition::Next("evaluate"))
            })
        }
    }

    struct EvaluateStep;

    impl WorkflowStep for EvaluateStep {
        fn id(&self) -> &'static str {
            "evaluate"
        }

        fn execute<'a>(
            &self,
            context: &'a mut agent_kernel::WorkflowContext,
        ) -> agent_kernel::BoxFuture<'a, Result<agent_kernel::StepTransition, agent_kernel::RunError>> {
            Box::pin(async move {
                let passed = {
                    let attempt = context.state::<AttemptCount>()?.0;
                    attempt > 0
                };
                context.insert_state(
                    QualityGate {
                        score: if passed { 90 } else { 40 },
                        passed,
                        reason: if passed {
                            "good".to_owned()
                        } else {
                            "needs work".to_owned()
                        },
                    }
                );
                
                if passed {
                    Ok(agent_kernel::StepTransition::Next("finalize"))
                } else {
                    let attempt = context.state_mut::<AttemptCount>()?;
                    if attempt.0 < 2 {
                        attempt.0 += 1;
                        Ok(agent_kernel::StepTransition::Next("refine"))
                    } else {
                        Ok(agent_kernel::StepTransition::Next("finalize"))
                    }
                }
            })
        }
    }

    struct RefineStep;

    impl WorkflowStep for RefineStep {
        fn id(&self) -> &'static str {
            "refine"
        }

        fn execute<'a>(
            &self,
            context: &'a mut agent_kernel::WorkflowContext,
        ) -> agent_kernel::BoxFuture<'a, Result<agent_kernel::StepTransition, agent_kernel::RunError>> {
            Box::pin(async move {
                let draft = context.state_mut::<String>()?;
                *draft = format!("{draft}-refined");
                Ok(agent_kernel::StepTransition::Next("evaluate"))
            })
        }
    }

    struct FinalizeStep;

    impl WorkflowStep for FinalizeStep {
        fn id(&self) -> &'static str {
            "finalize"
        }

        fn execute<'a>(
            &self,
            context: &'a mut agent_kernel::WorkflowContext,
        ) -> agent_kernel::BoxFuture<'a, Result<agent_kernel::StepTransition, agent_kernel::RunError>> {
            Box::pin(async move {
                let draft = context.state::<String>()?.clone();
                let gate = context.state::<QualityGate>()?.clone();

                let output = ArtifactEnvelope {
                    key: "result".into(),
                    kind: "text".into(),
                    value: serde_json::to_value(draft).unwrap()
                };

                Ok(agent_kernel::StepTransition::Complete {
                    output_artifact: Some(output),
                    qualified: gate.passed,
                })
            })
        }
    }

    #[tokio::test]
    async fn executor_retries_failed_quality_gates() {
        let executor = WorkflowExecutor::builder(Arc::new(EmptyServices), ExecutorSettings::default())
            .register_workflow(Arc::new(GateWorkflow))
            .build();

        let report = executor
            .run(agent_kernel::RunRequest {
                workflow: "tests.gate".to_owned(),
                input: serde_json::json!({}),
                options: RunOptions::default(),
            })
            .await
            .expect("workflow should succeed");

        assert!(report.qualified);
        let output: String = report.output().expect("result should decode");
        assert_eq!(output, "draft-0-refined");
    }

    struct RecordingStore {
        reports: Mutex<Vec<String>>,
    }

    impl ArtifactStore for RecordingStore {
        fn persist(
            &self,
            report: &agent_kernel::RunReport,
        ) -> agent_kernel::BoxFuture<'_, Result<(), agent_kernel::RunError>> {
            let workflow = report.workflow.clone();
            let reports = &self.reports;
            Box::pin(async move {
                reports.lock().expect("poisoned").push(workflow);
                Ok(())
            })
        }
    }

    struct ServicesWithStore {
        store: Arc<RecordingStore>,
    }

    impl CapabilityRegistry for ServicesWithStore {
        fn llm(&self, _name: &str) -> Result<Arc<dyn LanguageModel>, agent_kernel::RunError> {
            Err(agent_kernel::RunError::Internal("no llm configured".to_owned()))
        }

        fn source_fetcher(
            &self,
        ) -> Result<Arc<dyn SourceFetcher>, agent_kernel::RunError> {
            Err(agent_kernel::RunError::Internal("no fetcher configured".to_owned()))
        }

        fn search_provider(&self) -> Option<Arc<dyn SearchProvider>> {
            None
        }

        fn artifact_store(&self) -> Option<Arc<dyn ArtifactStore>> {
            Some(self.store.clone())
        }
    }

    #[tokio::test]
    async fn executor_persists_reports_when_store_is_available() {
        let store = Arc::new(RecordingStore {
            reports: Mutex::new(Vec::new()),
        });
        let executor = WorkflowExecutor::builder(
            Arc::new(ServicesWithStore {
                store: store.clone(),
            }),
            ExecutorSettings::default(),
        )
        .register_workflow(Arc::new(GateWorkflow))
        .build();

        executor
            .run(agent_kernel::RunRequest {
                workflow: "tests.gate".to_owned(),
                input: serde_json::json!({}),
                options: RunOptions::default(),
            })
            .await
            .expect("workflow should succeed");

        assert_eq!(store.reports.lock().expect("poisoned").len(), 1);
    }

    struct DummyParser;

    impl agent_kernel::DocumentParser<String> for DummyParser {
        fn parse_path(&self, path: &Path) -> Result<String, agent_kernel::RunError> {
            Ok(path.display().to_string())
        }
    }

    #[test]
    fn parser_trait_is_generic() {
        let parser = DummyParser;
        assert_eq!(
            parser
                .parse_path(Path::new("/tmp/example"))
                .expect("path should parse"),
            "/tmp/example"
        );
    }
}
