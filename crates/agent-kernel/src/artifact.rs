use crate::telemetry::{AgentTrajectory, Telemetry};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactEnvelope {
    pub key: String,
    pub kind: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub agent_role: String,
    pub qualified: bool,
    pub output_artifact: Option<ArtifactEnvelope>,
    pub artifacts: Vec<ArtifactEnvelope>,
    pub telemetry: Telemetry,
    pub trajectory: AgentTrajectory,
    pub total_duration_ms: u128,
}
