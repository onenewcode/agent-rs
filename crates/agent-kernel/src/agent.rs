use crate::{AgentTrajectory, SourceMaterial, Telemetry};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub task_goal: String,
    pub current_document: String,
    pub search_results: Vec<SourceMaterial>,
    pub feedback_history: Vec<AgentFeedback>,
    pub audit_log: Vec<AuditVerdict>,
}

impl AgentContext {
    #[must_use]
    pub fn new(task_goal: String, initial_doc: String) -> Self {
        Self {
            task_goal,
            current_document: initial_doc,
            search_results: Vec::new(),
            feedback_history: Vec::new(),
            audit_log: Vec::new(),
        }
    }
}

pub struct AgentSession {
    pub session_id: String,
    pub context: Arc<tokio::sync::RwLock<AgentContext>>,
    pub telemetry: Arc<tokio::sync::Mutex<Telemetry>>,
    pub trajectory: Arc<tokio::sync::Mutex<AgentTrajectory>>,
}
