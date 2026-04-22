use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Boxed error type for easy chaining and passing.
pub type BError = Box<Error>;
/// Result alias using BError.
pub type Result<T, E = BError> = std::result::Result<T, E>;

/// Structured error representing the "what", "where", and "how" of a failure.
#[derive(Debug, Error)]
pub enum Error {
    #[error("[{etype:?}] from {esource:?} (Retry: {retry:?}) {context}: {cause:?}")]
    Base {
        etype: ErrorType,
        esource: ErrorSource,
        retry: RetryType,
        context: String,
        #[source]
        cause: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorType {
    Parse,
    Config,
    Network,
    Provider,
    Timeout,
    Evaluation,
    Artifact,
    Internal,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorSource {
    Upstream,
    Downstream,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryType {
    Retry,
    Decide,
    Fatal,
}

impl Error {
    pub fn explain(etype: ErrorType, context: impl Into<String>) -> Self {
        Self::Base {
            etype,
            esource: ErrorSource::Internal,
            retry: RetryType::Fatal,
            context: context.into(),
            cause: None,
        }
    }

    pub fn because<E>(cause: E, etype: ErrorType, context: impl Into<String>) -> Self
    where
        E: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    {
        Self::Base {
            etype,
            esource: ErrorSource::Internal,
            retry: RetryType::Fatal,
            context: context.into(),
            cause: Some(cause.into()),
        }
    }

    pub fn set_source(mut self, new_source: ErrorSource) -> Self {
        let Self::Base { ref mut esource, .. } = self;
        *esource = new_source;
        self
    }

    pub fn set_retry(mut self, new_retry: RetryType) -> Self {
        let Self::Base { ref mut retry, .. } = self;
        *retry = new_retry;
        self
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Base { retry, .. } => *retry == RetryType::Retry,
        }
    }
}

/// Extension trait for ergonomic error conversion.
pub trait OrErr<T, E> {
    fn or_err(self, etype: ErrorType, context: &str) -> Result<T>;
}

impl<T, E> OrErr<T, E> for std::result::Result<T, E>
where
    E: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
{
    fn or_err(self, etype: ErrorType, context: &str) -> Result<T> {
        self.map_err(|e| Box::new(Error::because(e, etype, context.to_string())))
    }
}

pub trait OkOrErr<T> {
    fn or_err(self, etype: ErrorType, context: &str) -> Result<T>;
}

impl<T> OkOrErr<T> for Option<T> {
    fn or_err(self, etype: ErrorType, context: &str) -> Result<T> {
        self.ok_or_else(|| Box::new(Error::explain(etype, context.to_string())))
    }
}
