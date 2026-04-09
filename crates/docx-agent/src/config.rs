use std::{fs, path::Path};

use serde::Deserialize;
use tracing::info;

use crate::error::DocxAgentError;

#[derive(Debug, Clone, Deserialize)]
pub struct DocxAgentConfig {
    pub llm: LlmConfig,
    pub search: SearchConfig,
    pub limits: LimitsConfig,
    pub fetch: FetchConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    pub provider: String,
    pub api_key: Option<String>,
    pub max_results: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LimitsConfig {
    pub document_chars: usize,
    pub source_chars: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FetchConfig {
    pub user_agent: String,
}

impl DocxAgentConfig {
    pub fn from_path(path: &Path) -> Result<Self, DocxAgentError> {
        if !path.exists() {
            return Err(DocxAgentError::ConfigNotFound(path.display().to_string()));
        }

        let content = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        config.validate()?;
        info!(
            config = %path.display(),
            llm_provider = %config.llm.provider,
            llm_model = %config.llm.model,
            search_provider = %config.search.provider,
            search_enabled = config.search.api_key.is_some(),
            "loaded agent configuration"
        );
        Ok(config)
    }

    pub(crate) fn validate(&self) -> Result<(), DocxAgentError> {
        if self.llm.provider != "openrouter" {
            return Err(DocxAgentError::UnsupportedProvider {
                kind: "llm",
                provider: self.llm.provider.clone(),
            });
        }

        if self.search.provider != "tavily" {
            return Err(DocxAgentError::UnsupportedProvider {
                kind: "search",
                provider: self.search.provider.clone(),
            });
        }

        validate_secret("llm.api_key", &self.llm.api_key)?;

        if let Some(api_key) = &self.search.api_key {
            validate_secret("search.api_key", api_key)?;
        }

        Ok(())
    }
}

fn validate_secret(field: &'static str, value: &str) -> Result<(), DocxAgentError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(DocxAgentError::InvalidConfigValue {
            field,
            reason: "value must not be empty",
        });
    }

    if trimmed.starts_with("replace-with-") {
        return Err(DocxAgentError::InvalidConfigValue {
            field,
            reason: "placeholder value must be replaced before running",
        });
    }

    Ok(())
}
