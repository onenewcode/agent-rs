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
            context: Arc::clone(&context),
            telemetry: Arc::clone(&telemetry),
            trajectory: Arc::clone(&trajectory),
        };

        for i in 0..self.max_iterations {
            self.execute_iteration(i + 1, &session).await?;

            let ctx = context.write().await;
            if let Some(feedback) = ctx.feedback_history.last() {
                if feedback.passed {
                    tracing::info!(score = feedback.score, "Reviewer passed the document. Collaboration complete.");
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

    async fn execute_iteration(&self, iteration: usize, session: &AgentSession) -> Result<()> {
        tracing::info!(iteration, "Starting Writer turn");
        self.run_agent_with_retry(self.writer.as_ref(), session, iteration).await?;

        tracing::info!(iteration, "Starting Reviewer turn");
        self.run_agent_with_retry(self.reviewer.as_ref(), session, iteration).await?;

        // Audit the turn
        let verdict = self.auditor.audit_turn(session).await?;
        let mut ctx = session.context.write().await;

        if let Some(ref correction) = verdict.suggestion_for_writer {
            tracing::warn!(iteration, "Auditor detected communication stall: {}", correction);
            if !ctx.task_goal.contains("CRITICAL: You are ignoring") {
                ctx.task_goal = format!("{}\n\n{}", correction, ctx.task_goal);
            }
        }
        ctx.audit_log.push(verdict);
        Ok(())
    }

    async fn run_agent_with_retry(
        &self,
        agent: &dyn AutonomousAgent,
        session: &AgentSession,
        iteration: usize,
    ) -> Result<()> {
        retry_with_backoff(
            &format!("{} turn (iteration {iteration})", agent.role()),
            &self.retry_policy,
            || agent.run(session),
            |e| e.is_retryable(),
        )
        .await
        .map_err(|e| {
            tracing::error!(agent = agent.role(), iteration, error = %e, "Agent failed critically");
            Box::new(Error::Base {
                etype: ErrorType::Provider,
                esource: ErrorSource::Internal,
                retry: RetryType::Fatal,
                context: format!("{} agent failed: {e}", agent.role()),
                cause: Some(e),
            })
        })
    }
}
