use crate::generate_run_id;
use agent_kernel::{
    AgentAuditor, AgentContext, AgentSession, AgentTrajectory, AutonomousAgent, Error, ErrorSource,
    ErrorType, Result, RetryType, RunReport, Telemetry,
};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::retry::{RetryPolicy, retry_with_backoff};

pub struct AgentOrchestrator {
    writer: Arc<dyn AutonomousAgent>,
    reviewer: Arc<dyn AutonomousAgent>,
    auditor: Arc<dyn AgentAuditor>,
    max_iterations: usize,
    retry_policy: RetryPolicy,
}

impl AgentOrchestrator {
    pub fn new(
        writer: Arc<dyn AutonomousAgent>,
        reviewer: Arc<dyn AutonomousAgent>,
        auditor: Arc<dyn AgentAuditor>,
        max_iterations: usize,
    ) -> Self {
        Self {
            writer,
            reviewer,
            auditor,
            max_iterations,
            retry_policy: RetryPolicy::default(),
        }
    }

    #[must_use]
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub async fn run(&self, task_goal: String, initial_doc: String) -> Result<(RunReport, String)> {
        let start_time = std::time::Instant::now();
        let session_id = generate_run_id("mas.expansion");
        let context = Arc::new(RwLock::new(AgentContext::new(task_goal, initial_doc)));
        let telemetry = Arc::new(Mutex::new(Telemetry::default()));
        let trajectory = Arc::new(Mutex::new(AgentTrajectory::default()));

        let session = AgentSession {
            session_id: session_id.clone(),
            context: context.clone(),
            telemetry: telemetry.clone(),
            trajectory: trajectory.clone(),
        };

        for i in 0..self.max_iterations {
            let iteration = i + 1;
            tracing::info!(iteration, "Starting Writer turn");

            retry_with_backoff(
                &format!("Writer turn (iteration {iteration})"),
                &self.retry_policy,
                || self.writer.run(&session),
                agent_kernel::Error::is_retryable,
            )
            .await
            .map_err(|e| {
                tracing::error!(agent = self.writer.role(), iteration, error = %e, "Writer agent failed critically");
                Box::new(Error::explain(ErrorType::Provider, format!("{} agent failed: {e}", self.writer.role()))
                    .set_source(ErrorSource::Internal)
                    .set_retry(RetryType::Fatal))
            })?;

            tracing::info!(iteration, "Starting Reviewer turn");

            retry_with_backoff(
                &format!("Reviewer turn (iteration {iteration})"),
                &self.retry_policy,
                || self.reviewer.run(&session),
                agent_kernel::Error::is_retryable,
            )
            .await
            .map_err(|e| {
                tracing::error!(agent = self.reviewer.role(), iteration, error = %e, "Reviewer agent failed critically");
                Box::new(Error::explain(ErrorType::Provider, format!("{} agent failed: {e}", self.reviewer.role()))
                    .set_source(ErrorSource::Internal)
                    .set_retry(RetryType::Fatal))
            })?;

            // Audit the turn
            let verdict = self.auditor.audit_turn(&session).await?;
            let mut ctx = context.write().await;

            // If the auditor suggested a correction, prepend it to the task goal for the next iteration
            if let Some(ref correction) = verdict.suggestion_for_writer {
                tracing::warn!(
                    iteration,
                    "Auditor detected communication stall: {}",
                    correction
                );
                if !ctx.task_goal.contains("CRITICAL: You are ignoring") {
                    ctx.task_goal = format!("{}\n\n{}", correction, ctx.task_goal);
                }
            }

            ctx.audit_log.push(verdict);

            if let Some(feedback) = ctx.feedback_history.last() {
                if feedback.passed {
                    let score = feedback.score;
                    tracing::info!(score, "Reviewer passed the document. Collaboration complete.");
                    break;
                }
            }
        }

        let ctx = context.read().await;
        let final_doc = ctx.current_document.clone();
        let tel = *telemetry.lock().await;
        let traj = trajectory.lock().await.clone();

        let audit_report = self.auditor.generate_final_report(&ctx);
        tracing::info!(
            efficiency = audit_report.communication_efficiency,
            iterations = audit_report.total_iterations,
            "Audit complete"
        );

        let report = RunReport {
            run_id: session_id,
            agent_role: "CollaborativePair".to_owned(),
            qualified: ctx.feedback_history.last().is_some_and(|f| f.passed),
            telemetry: tel,
            trajectory: traj,
            total_duration_ms: start_time.elapsed().as_millis(),
        };

        Ok((report, final_doc))
    }
}
