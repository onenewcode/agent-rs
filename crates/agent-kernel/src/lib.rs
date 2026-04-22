#![allow(clippy::missing_errors_doc)]

pub mod agent;
pub mod artifact;
pub mod source;
pub mod telemetry;
pub mod traits;
pub mod typemap;

pub use agent::*;
pub use agent::{StepOutcome, WorkflowContext};
pub use artifact::*;
pub use source::*;
pub use telemetry::*;
pub use traits::*;
pub use typemap::*;

pub mod error {
    pub use agent_error::{
        Error as AgentError, ErrorSource, ErrorType, OkOrErr, OrErr, RetryType,
    };
    pub type Result<T> = std::result::Result<T, AgentError>;
}

pub use error::{AgentError, ErrorType, OkOrErr, OrErr, Result, RetryType};

#[must_use]
pub fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[must_use]
pub fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
