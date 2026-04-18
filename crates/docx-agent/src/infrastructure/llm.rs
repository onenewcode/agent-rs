use std::time::Duration;

use rig::{client::CompletionClient, completion::Prompt, providers::openrouter};
use tracing::{info, warn};

use crate::{config::DocxAgentConfig, error::DocxAgentError};

const RETRYABLE_ERROR_PATTERNS: &[&str] = &[
    "429",
    "rate limit",
    "rate-limited",
    "rate limited",
    "too many requests",
    "temporarily rate-limited",
    "timeout",
    "timed out",
    "temporarily unavailable",
    "service unavailable",
    "connection reset",
    "deadline exceeded",
];

pub(crate) fn build_agent(
    http: &reqwest::Client,
    config: &DocxAgentConfig,
) -> Result<impl Prompt, DocxAgentError> {
    let client = openrouter::Client::builder()
        .api_key(config.llm.api_key.as_str())
        .http_client(http.clone())
        .build()
        .map_err(|error| {
            DocxAgentError::Agent(format!("failed to build OpenRouter client: {error}").into())
        })?;

    Ok(client
        .agent(&config.llm.model)
        .preamble(config.system_prompt())
        .build())
}

pub(crate) async fn generate_with_retry(
    agent: &impl Prompt,
    prompt: &str,
    model: &str,
    max_attempts: usize,
) -> Result<String, DocxAgentError> {
    for attempt in 1..=max_attempts {
        match agent.prompt(prompt).await {
            Ok(content) => {
                info!(
                    model,
                    attempt,
                    output_chars = content.chars().count(),
                    "received generation response from OpenRouter"
                );
                return Ok(content);
            }
            Err(error) => {
                let message = error.to_string();
                if is_retryable_error(&message) && attempt < max_attempts {
                    let delay = Duration::from_secs(attempt as u64 * 2);
                    warn!(
                        model,
                        attempt,
                        delay_secs = delay.as_secs(),
                        error = %message,
                        "OpenRouter request failed with a retryable error"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }

                return Err(DocxAgentError::Agent(message.into()));
            }
        }
    }

    Err(DocxAgentError::Agent(
        "OpenRouter generation exhausted retries".to_owned().into(),
    ))
}

fn is_retryable_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    RETRYABLE_ERROR_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::is_retryable_error;

    #[test]
    fn retryable_error_detection_covers_common_variants() {
        assert!(is_retryable_error("HTTP 429 Too Many Requests"));
        assert!(is_retryable_error("Provider is Rate Limited upstream"));
        assert!(is_retryable_error("request timed out"));
        assert!(!is_retryable_error("invalid api key"));
    }
}
