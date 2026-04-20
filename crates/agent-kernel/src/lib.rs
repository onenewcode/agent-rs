#![allow(clippy::missing_errors_doc)]

pub mod agent;
pub mod artifact;
pub mod error;
pub mod source;
pub mod telemetry;
pub mod traits;

pub use agent::*;
pub use artifact::*;
pub use error::*;
pub use source::*;
pub use telemetry::*;
pub use traits::*;

#[must_use]
pub fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[must_use]
pub fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
