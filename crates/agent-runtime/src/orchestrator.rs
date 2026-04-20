use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use agent_kernel::{
    AgentContext, AgentSession, AgentTrajectory, AutonomousAgent, RunError, RunReport,
    Telemetry,
};
use crate::generate_run_id;

pub struct AgentOrchestrator {
    writer: Arc<dyn AutonomousAgent>,
    reviewer: Arc<dyn AutonomousAgent>,
    max_iterations: usize,
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
        }
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
            tracing::info!(iteration = i + 1, "Starting Writer turn");
            self.writer.run(&session).await?;

            tracing::info!(iteration = i + 1, "Starting Reviewer turn");
            self.reviewer.run(&session).await?;

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
            output_artifact: None,
            artifacts: Vec::new(),
            telemetry: tel,
            trajectory: traj,
            total_duration_ms: start_time.elapsed().as_millis(),
        };

        Ok((report, final_doc))
    }
}
