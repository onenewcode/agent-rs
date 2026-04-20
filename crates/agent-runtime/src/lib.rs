#![allow(clippy::missing_errors_doc)]

pub mod orchestrator;
pub mod retry;

pub use orchestrator::AgentOrchestrator;
pub use retry::{RetryPolicy, retry_with_backoff};

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
