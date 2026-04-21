use serde::{Deserialize, Serialize};
use std::error::Error as StdError;
use std::fmt;

/// Boxed error type for easy chaining and passing.
pub type BError = Box<Error>;
/// Result alias using BError.
pub type Result<T, E = BError> = std::result::Result<T, E>;

/// Structured error representing the "what", "where", and "how" of a failure.
#[derive(Debug)]
pub struct Error {
    pub etype: ErrorType,
    pub esource: ErrorSource,
    pub retry: RetryType,
    pub cause: Option<Box<dyn StdError + Send + Sync + 'static>>,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorType {
    Parse,
    Config,
    Network,
    Provider,
    Timeout,
    Evaluation,
    Workflow,
    Artifact,
    Internal,
    Tool,
    Custom,
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
    pub fn new(etype: ErrorType, esource: ErrorSource, retry: RetryType) -> Self {
        Self {
            etype,
            esource,
            retry,
            cause: None,
            context: None,
        }
    }

    pub fn because<E>(cause: E, etype: ErrorType, context: String) -> Self
    where
        E: Into<Box<dyn StdError + Send + Sync + 'static>>,
    {
        Self {
            etype,
            esource: ErrorSource::Internal, // Default, can be adjusted
            retry: RetryType::Fatal,        // Default, can be adjusted
            cause: Some(cause.into()),
            context: Some(context),
        }
    }

    pub fn explain(etype: ErrorType, context: String) -> Self {
        Self {
            etype,
            esource: ErrorSource::Internal,
            retry: RetryType::Fatal,
            cause: None,
            context: Some(context),
        }
    }

    pub fn set_source(mut self, source: ErrorSource) -> Self {
        self.esource = source;
        self
    }

    pub fn set_retry(mut self, retry: RetryType) -> Self {
        self.retry = retry;
        self
    }

    pub fn is_retryable(&self) -> bool {
        self.retry == RetryType::Retry
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.cause
            .as_ref()
            .map(|e| e.as_ref() as &(dyn StdError + 'static))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{:?}] from {:?} (Retry: {:?})",
            self.etype, self.esource, self.retry
        )?;
        if let Some(ctx) = &self.context {
            write!(f, ": {}", ctx)?;
        }
        if let Some(cause) = &self.cause {
            write!(f, " | Caused by: {}", cause)?;
        }
        Ok(())
    }
}

/// Extension trait for ergonomic error conversion.
pub trait OrErr<T, E> {
    fn or_err(self, etype: ErrorType, context: &str) -> Result<T>;
}

impl<T, E> OrErr<T, E> for std::result::Result<T, E>
where
    E: Into<Box<dyn StdError + Send + Sync + 'static>>,
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

/// Extension trait for adding context to existing errors.
pub trait Context<T> {
    fn err_context(self, context: &str) -> Result<T>;
}

impl<T> Context<T> for Result<T> {
    fn err_context(mut self, context: &str) -> Result<T> {
        if let Err(ref mut e) = self {
            if let Some(ref mut existing) = e.context {
                existing.push_str(": ");
                existing.push_str(context);
            } else {
                e.context = Some(context.to_string());
            }
        }
        self
    }
}
