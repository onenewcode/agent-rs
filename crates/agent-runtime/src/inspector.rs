use agent_kernel::{AgentAuditor, AgentContext, AgentSession, AuditReport, AuditVerdict, BoxFuture, Result};

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
        session: &'a AgentSession,
    ) -> BoxFuture<'a, Result<AuditVerdict>> {
        Box::pin(async move {
            let context = session.context.read().await;
            
            let mut ignored_feedback = Vec::new();
            let mut is_stalled = false;
            
            // Heuristic for ignored feedback: 
            // If the latest feedback has identical suggestions to the previous round, 
            // the writer is likely ignoring the reviewer.
            if context.feedback_history.len() >= 2 {
                let current = &context.feedback_history[context.feedback_history.len() - 1];
                let previous = &context.feedback_history[context.feedback_history.len() - 2];
                
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

            Ok(AuditVerdict {
                is_stalled,
                ignored_feedback,
                suggestion_for_writer,
            })
        })
    }

    fn generate_final_report(
        &self,
        context: &AgentContext,
    ) -> AuditReport {
        let mut communication_efficiency = 1.0;
        let mut convergence_issues = Vec::new();
        
        let stall_count = context.audit_log.iter().filter(|v| v.is_stalled).count();
        if !context.audit_log.is_empty() {
            communication_efficiency = 1.0 - (stall_count as f32 / context.audit_log.len() as f32);
        }
        
        for verdict in &context.audit_log {
            for issue in &verdict.ignored_feedback {
                if !convergence_issues.contains(issue) {
                    convergence_issues.push(issue.clone());
                }
            }
        }

        AuditReport {
            total_iterations: context.feedback_history.len(),
            communication_efficiency,
            convergence_issues,
        }
    }
}
