use crate::generate_run_id;
use crate::{CollaborationStrategy, RetryPolicy, retry_with_backoff};
use agent_kernel::{
    AgentAuditor, AgentTrajectory, AuditLog, AuditReport, AuditorFeedbackList, AutonomousAgent,
    FeedbackHistory, Result, RunReport, StepOutcome, TaskGoal, Telemetry, TokenUsage,
    WorkflowContext,
};
use std::sync::Arc;

pub struct SequentialWriterReviewerAuditor {
    writer: Arc<dyn AutonomousAgent>,
    reviewer: Arc<dyn AutonomousAgent>,
    auditor: Arc<dyn AgentAuditor>,
    retry_policy: RetryPolicy,
}

impl SequentialWriterReviewerAuditor {
    pub fn new(
        writer: Arc<dyn AutonomousAgent>,
        reviewer: Arc<dyn AutonomousAgent>,
        auditor: Arc<dyn AgentAuditor>,
        retry_policy: RetryPolicy,
    ) -> Self {
        Self {
            writer,
            reviewer,
            auditor,
            retry_policy,
        }
    }

    async fn run_agent_with_retry(
        &self,
        agent: &dyn AutonomousAgent,
        context: Arc<WorkflowContext>,
        iteration: usize,
    ) -> Result<StepOutcome> {
        retry_with_backoff(
            &format!("{} turn (iteration {iteration})", agent.role()),
            &self.retry_policy,
            || agent.run(Arc::clone(&context)),
            |e| e.is_retryable(),
        )
        .await
        .map_err(|e| {
            tracing::error!(agent = agent.role(), iteration, error = %e, "Agent failed critically");
            e
        })
    }

    fn accumulate_outcome(
        &self,
        total_usage: &mut TokenUsage,
        all_events: &mut Vec<agent_kernel::TrajectoryStep>,
        outcome: &StepOutcome,
    ) {
        if let Some(usage) = outcome.usage {
            total_usage.prompt_tokens += usage.prompt_tokens;
            total_usage.completion_tokens += usage.completion_tokens;
            total_usage.total_tokens += usage.total_tokens;
        }
        all_events.extend(outcome.trajectory_events.clone());
    }
}

impl CollaborationStrategy for SequentialWriterReviewerAuditor {
    fn execute_iteration<'a>(
        &'a self,
        iteration: usize,
        current_ctx: Arc<WorkflowContext>,
    ) -> agent_kernel::BoxFuture<'a, Result<StepOutcome>> {
        Box::pin(async move {
            let mut total_usage = TokenUsage::default();
            let mut trajectory_events = Vec::new();

            // 1. Writer Turn
            tracing::info!(iteration, "Starting Writer turn");
            let writer_outcome = self
                .run_agent_with_retry(self.writer.as_ref(), Arc::clone(&current_ctx), iteration)
                .await?;
            self.accumulate_outcome(&mut total_usage, &mut trajectory_events, &writer_outcome);
            let current_ctx = Arc::new(writer_outcome.updated_context);

            // 2. Reviewer Turn
            tracing::info!(iteration, "Starting Reviewer turn");
            let reviewer_outcome = self
                .run_agent_with_retry(self.reviewer.as_ref(), Arc::clone(&current_ctx), iteration)
                .await?;
            self.accumulate_outcome(&mut total_usage, &mut trajectory_events, &reviewer_outcome);
            let current_ctx = Arc::new(reviewer_outcome.updated_context);

            // 3. Auditor Turn
            tracing::info!(iteration, "Starting Auditor turn");
            let audit_outcome = self.auditor.audit_turn(Arc::clone(&current_ctx)).await?;

            // Handle auditor feedback without mutating TaskGoal
            let mut final_ctx = audit_outcome.updated_context.clone();
            if let Some(log) = final_ctx.state.get::<AuditLog>() {
                if let Some(verdict) = log.0.last() {
                    if let Some(ref correction) = verdict.suggestion_for_writer {
                        tracing::warn!(
                            iteration,
                            "Auditor detected communication stall: {}",
                            correction
                        );
                        let mut feedback_list = final_ctx
                            .state
                            .get::<AuditorFeedbackList>()
                            .cloned()
                            .unwrap_or_default();
                        feedback_list.0.push(correction.clone());
                        final_ctx.state.insert(feedback_list);
                    }
                }
            }

            self.accumulate_outcome(&mut total_usage, &mut trajectory_events, &audit_outcome);

            Ok(StepOutcome {
                updated_context: final_ctx,
                usage: Some(total_usage),
                trajectory_events,
            })
        })
    }
}

pub struct AgentOrchestrator {
    strategy: Arc<dyn CollaborationStrategy>,
    auditor: Arc<dyn AgentAuditor>, // Keep for final report
    max_iterations: usize,
}

impl AgentOrchestrator {
    pub fn new(
        writer: Arc<dyn AutonomousAgent>,
        reviewer: Arc<dyn AutonomousAgent>,
        auditor: Arc<dyn AgentAuditor>,
        max_iterations: usize,
    ) -> Self {
        let strategy = Arc::new(SequentialWriterReviewerAuditor::new(
            writer,
            reviewer,
            Arc::clone(&auditor),
            RetryPolicy::default(),
        ));

        Self {
            strategy,
            auditor,
            max_iterations,
        }
    }

    #[must_use]
    pub fn with_strategy(mut self, strategy: Arc<dyn CollaborationStrategy>) -> Self {
        self.strategy = strategy;
        self
    }

    pub async fn run(&self, task_goal: String, initial_doc: String) -> Result<(RunReport, String)> {
        let start_time = std::time::Instant::now();
        let session_id = generate_run_id("mas.expansion");

        let mut context = WorkflowContext::default();
        context.state.insert(TaskGoal(task_goal));
        context.state.insert(initial_doc);
        context.state.insert(FeedbackHistory::default());
        context.state.insert(AuditLog::default());
        context.state.insert(AuditorFeedbackList::default());

        let mut current_ctx = Arc::new(context);
        let mut telemetry = Telemetry::default();
        let mut trajectory = AgentTrajectory::default();

        for i in 0..self.max_iterations {
            let iteration = i + 1;
            let outcome = self
                .strategy
                .execute_iteration(iteration, Arc::clone(&current_ctx))
                .await?;

            current_ctx = Arc::new(outcome.updated_context);
            if let Some(usage) = outcome.usage {
                // For the orchestrator, we use a generic model name for the aggregate usage
                // In a real scenario, we would pass the actual model ID from each step
                telemetry.add_usage("orchestrator:aggregate", usage);
            }
            trajectory.steps.extend(outcome.trajectory_events);

            let ctx = current_ctx.as_ref();
            if let Some(history) = ctx.state.get::<FeedbackHistory>() {
                if let Some(feedback) = history.0.last() {
                    if feedback.passed {
                        tracing::info!(
                            score = feedback.score,
                            "Reviewer passed the document. Collaboration complete."
                        );
                        break;
                    }
                }
            }
        }

        let ctx = current_ctx.as_ref();
        let final_doc = ctx.state.get::<String>().cloned().unwrap_or_default();
        let audit_report = self.auditor.generate_final_report(ctx);

        self.finalize_audit(&audit_report);

        let report = RunReport {
            run_id: session_id,
            agent_role: "CollaborativePair".to_owned(),
            qualified: ctx
                .state
                .get::<FeedbackHistory>()
                .and_then(|h| h.0.last())
                .is_some_and(|f| f.passed),
            telemetry,
            trajectory,
            total_duration_ms: start_time.elapsed().as_millis(),
        };

        Ok((report, final_doc))
    }

    fn finalize_audit(&self, report: &AuditReport) {
        tracing::info!(
            efficiency = report.communication_efficiency,
            iterations = report.total_iterations,
            "Audit complete"
        );
    }
}
