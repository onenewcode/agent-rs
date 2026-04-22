#![allow(clippy::missing_errors_doc)]

pub mod inspector;
pub mod orchestrator;
pub mod retry;

pub use inspector::DialogueInspector;
pub use orchestrator::AgentOrchestrator;
pub use retry::{RetryPolicy, retry_with_backoff};

use agent_kernel::{Result, StepOutcome, WorkflowContext};
use std::sync::Arc;

/// Strategy for multi-agent collaboration and orchestration.
pub trait CollaborationStrategy: Send + Sync {
    /// Executes a single iteration of the collaboration.
    fn execute_iteration<'a>(
        &'a self,
        iteration: usize,
        context: Arc<WorkflowContext>,
    ) -> agent_kernel::BoxFuture<'a, Result<StepOutcome>>;
}

use std::time::{SystemTime, UNIX_EPOCH};

#[must_use]
pub fn generate_run_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{prefix}-{millis}")
}

#[must_use]
pub fn sanitize_error_msg(err: &impl std::fmt::Display) -> String {
    let s = err.to_string();
    let trimmed = s.trim();

    if trimmed.contains("\n\n\n") {
        let mut lines = Vec::new();
        for line in trimmed.lines() {
            let line_trimmed = line.trim();
            if !line_trimmed.is_empty() {
                lines.push(line_trimmed);
            }
        }
        return lines.join(" | ");
    }

    trimmed.to_owned()
}
