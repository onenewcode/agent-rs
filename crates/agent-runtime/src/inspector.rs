use agent_kernel::{
    AgentAuditor, AuditLog, AuditReport, AuditVerdict, BoxFuture, FeedbackHistory, Result,
    StepOutcome, WorkflowContext,
};
use std::sync::Arc;

pub struct DialogueInspector;

impl DialogueInspector {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for DialogueInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAuditor for DialogueInspector {
    fn audit_turn<'a>(
        &'a self,
        context: Arc<WorkflowContext>,
    ) -> BoxFuture<'a, Result<StepOutcome>> {
        Box::pin(async move {
            let history = context
                .state
                .get::<FeedbackHistory>()
                .cloned()
                .unwrap_or_default();

            let mut ignored_feedback = Vec::new();
            let mut is_stalled = false;

            if history.0.len() >= 2 {
                let current = &history.0[history.0.len() - 1];
                let previous = &history.0[history.0.len() - 2];

                for suggestion in &current.suggestions {
                    if previous.suggestions.contains(suggestion) {
                        ignored_feedback.push(suggestion.clone());
                    }
                }
            }

            if !ignored_feedback.is_empty() {
                is_stalled = true;
            }

            let suggestion_for_writer = if is_stalled {
                Some(format!(
                    "CRITICAL: You are ignoring the following reviewer feedback: {}. \
                    Address these points immediately to proceed.",
                    ignored_feedback.join(", ")
                ))
            } else {
                None
            };

            let verdict = AuditVerdict {
                is_stalled,
                ignored_feedback,
                suggestion_for_writer,
            };

            let mut next_context = (*context).clone();
            let mut log = next_context
                .state
                .get::<AuditLog>()
                .cloned()
                .unwrap_or_default();
            log.0.push(verdict);
            next_context.state.insert(log);

            Ok(StepOutcome {
                updated_context: next_context,
                usage: None,
                trajectory_events: Vec::new(),
            })
        })
    }

    fn generate_final_report(&self, context: &WorkflowContext) -> AuditReport {
        let history = context
            .state
            .get::<FeedbackHistory>()
            .cloned()
            .unwrap_or_default();
        let audit_log = context.state.get::<AuditLog>().cloned().unwrap_or_default();

        let mut communication_efficiency = 1.0;
        let mut convergence_issues = Vec::new();

        let stall_count = audit_log.0.iter().filter(|v| v.is_stalled).count();
        if !audit_log.0.is_empty() {
            communication_efficiency = 1.0 - (stall_count as f32 / audit_log.0.len() as f32);
        }

        for verdict in &audit_log.0 {
            for issue in &verdict.ignored_feedback {
                if !convergence_issues.contains(issue) {
                    convergence_issues.push(issue.clone());
                }
            }
        }

        AuditReport {
            total_iterations: history.0.len(),
            communication_efficiency,
            convergence_issues,
        }
    }
}
