use crate::{AgentTrajectory, SourceMaterial, Telemetry};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentFeedback {
    pub score: u8,
    pub passed: bool,
    pub suggestions: Vec<String>,
    pub critical_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    pub task_goal: String,
    pub current_document: String,
    pub search_results: Vec<SourceMaterial>,
    pub feedback_history: Vec<AgentFeedback>,
}

impl AgentContext {
    #[must_use]
    pub fn new(task_goal: String, initial_doc: String) -> Self {
        Self {
            task_goal,
            current_document: initial_doc,
            search_results: Vec::new(),
            feedback_history: Vec::new(),
        }
    }
}

pub struct AgentSession {
    pub session_id: String,
    pub context: Arc<tokio::sync::RwLock<AgentContext>>,
    pub telemetry: Arc<tokio::sync::Mutex<Telemetry>>,
    pub trajectory: Arc<tokio::sync::Mutex<AgentTrajectory>>,
}
