use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use agent_kernel::{
    AgentContext, AgentSession, AgentTrajectory, AutonomousAgent, RunError, RunReport,
    Telemetry,
};
use crate::generate_run_id;

use crate::retry::{RetryPolicy, retry_with_backoff};

pub struct AgentOrchestrator {
    writer: Arc<dyn AutonomousAgent>,
    reviewer: Arc<dyn AutonomousAgent>,
    max_iterations: usize,
    retry_policy: RetryPolicy,
}

impl AgentOrchestrator {
    pub fn new(
        writer: Arc<dyn AutonomousAgent>,
        reviewer: Arc<dyn AutonomousAgent>,
        max_iterations: usize,
    ) -> Self {
        Self {
            writer,
            reviewer,
            max_iterations,
            retry_policy: RetryPolicy::default(),
        }
    }

    #[must_use]
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub async fn run(
        &self,
        task_goal: String,
        initial_doc: String,
    ) -> Result<(RunReport, String), RunError> {
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
                agent_kernel::RunError::is_retryable,
            )
            .await
            .map_err(|e| {
                tracing::error!(agent = self.writer.role(), iteration, error = %e, "Writer agent failed critically");
                RunError::Provider(format!("{} agent failed: {}", self.writer.role(), e))
            })?;

            tracing::info!(iteration, "Starting Reviewer turn");

            retry_with_backoff(
                &format!("Reviewer turn (iteration {iteration})"),
                &self.retry_policy,
                || self.reviewer.run(&session),
                agent_kernel::RunError::is_retryable,
            )
            .await
            .map_err(|e| {
                tracing::error!(agent = self.reviewer.role(), iteration, error = %e, "Reviewer agent failed critically");
                RunError::Provider(format!("{} agent failed: {}", self.reviewer.role(), e))
            })?;

            let ctx = context.read().await;
            if ctx.feedback_history.last().is_some_and(|f| f.passed) {
                tracing::info!("Reviewer passed the document. Collaboration complete.");
                break;
            }
        }

        let ctx = context.read().await;
        let final_doc = ctx.current_document.clone();
        let tel = telemetry.lock().await.clone();
        let traj = trajectory.lock().await.clone();
        
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
