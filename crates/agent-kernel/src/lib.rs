#![allow(clippy::missing_errors_doc)]

pub mod agent;
pub mod artifact;
pub mod error;
pub mod options;
pub mod source;
pub mod telemetry;
pub mod traits;

pub use agent::*;
pub use artifact::*;
pub use error::*;
pub use options::*;
pub use source::*;
pub use telemetry::*;
pub use traits::*;

use serde::Serialize;
use serde::de::DeserializeOwned;

impl RunReport {
    pub fn output<T: DeserializeOwned>(&self) -> Result<T, RunError> {
        let artifact = self.output_artifact.as_ref().ok_or_else(|| {
            RunError::Artifact("agent task completed without an output artifact".to_owned())
        })?;
        serde_json::from_value(artifact.value.clone()).map_err(|error| {
            RunError::Artifact(format!(
                "failed to decode output artifact `{}`: {error}",
                artifact.key
            ))
        })
    }
}

pub fn encode_artifact<T: Serialize>(
    key: impl Into<String>,
    kind: impl Into<String>,
    value: &T,
) -> Result<ArtifactEnvelope, RunError> {
    let key = key.into();
    let kind = kind.into();
    let serialized = serde_json::to_value(value)
        .map_err(|error| RunError::Artifact(format!("failed to serialize `{key}`: {error}")))?;
    Ok(ArtifactEnvelope {
        key,
        kind,
        value: serialized,
    })
}

#[must_use]
pub fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[must_use]
pub fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
