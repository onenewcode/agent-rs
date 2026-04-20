use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("timeout error: {0}")]
    Timeout(String),
    #[error("evaluation error: {0}")]
    Evaluation(String),
    #[error("workflow error: {0}")]
    Workflow(String),
    #[error("artifact error: {0}")]
    Artifact(String),
    #[error("internal error: {0}")]
    Internal(String),
}
