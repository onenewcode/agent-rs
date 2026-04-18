use thiserror::Error;

#[derive(Debug, Error)]
pub enum DocxAgentError {
    #[error("config file not found: {0}")]
    ConfigNotFound(String),
    #[error("config parse error: {0}")]
    ConfigParse(#[from] toml::de::Error),
    #[error("unsupported provider `{provider}` for {kind}")]
    UnsupportedProvider {
        kind: &'static str,
        provider: String,
    },
    #[error("invalid config value for `{field}`: {reason}")]
    InvalidConfigValue {
        field: &'static str,
        reason: &'static str,
    },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("xml parse error: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unsupported response content type: {0}")]
    UnsupportedContentType(String),
    #[error("document is empty after parsing")]
    EmptyDocument,
    #[error("research error ({kind}): {message}")]
    ResearchError {
        kind: &'static str,
        message: String,
    },
    #[error("agent execution failed: {0}")]
    Agent(#[from] agent_core::ExpansionError),
}
