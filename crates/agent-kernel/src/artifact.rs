use crate::telemetry::{AgentTrajectory, Telemetry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub agent_role: String,
    pub qualified: bool,
    pub telemetry: Telemetry,
    pub trajectory: AgentTrajectory,
    pub total_duration_ms: u128,
}
