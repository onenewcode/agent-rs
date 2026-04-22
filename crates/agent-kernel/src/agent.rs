use crate::telemetry::{TokenUsage, TrajectoryStep};
use crate::typemap::TypeMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFeedback {
    /// Score from 0 to 100
    pub score: u8,
    pub passed: bool,
    pub suggestions: Vec<String>,
    pub critical_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditVerdict {
    pub is_stalled: bool,
    pub ignored_feedback: Vec<String>,
    pub suggestion_for_writer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    pub total_iterations: usize,
    pub communication_efficiency: f32,
    pub convergence_issues: Vec<String>,
}

#[derive(Clone, Default)]
pub struct WorkflowContext {
    pub state: TypeMap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskGoal(pub String);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeedbackHistory(pub Vec<AgentFeedback>);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditLog(pub Vec<AuditVerdict>);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditorFeedbackList(pub Vec<String>);

pub struct StepOutcome {
    pub updated_context: WorkflowContext,
    pub usage: Option<TokenUsage>,
    pub trajectory_events: Vec<TrajectoryStep>,
}
