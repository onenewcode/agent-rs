use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOptions {
    pub global_timeout_secs: u64,
    pub capture_artifacts: bool,
}

impl RunOptions {
    #[must_use]
    pub fn with_defaults(global_timeout_secs: u64, capture_artifacts: bool) -> Self {
        Self {
            global_timeout_secs,
            capture_artifacts,
        }
    }
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            global_timeout_secs: 180,
            capture_artifacts: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRequest {
    pub workflow: String,
    pub input: Value,
    #[serde(default)]
    pub options: RunOptions,
}
