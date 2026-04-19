use std::time::Duration;

use agent_kernel::RunError;
use rig::{client::CompletionClient, completion::Prompt, providers::openrouter};
use tracing::{info, warn};

use crate::LlmProviderConfig;

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

pub struct OpenRouterModel<P: Prompt> {
    agent: P,
    config: LlmProviderConfig,
}

#[allow(clippy::needless_pass_by_value)]
pub fn build_openrouter_model(
    http: reqwest::Client,
    config: LlmProviderConfig,
    system_prompt: String,
) -> Result<OpenRouterModel<impl Prompt>, RunError> {
    let client = openrouter::Client::builder()
        .api_key(config.api_key.as_str())
        .http_client(http)
        .build()
        .map_err(|error| {
            RunError::Provider(format!("failed to build OpenRouter client: {error}"))
        })?;
    let agent = client.agent(&config.model).preamble(&system_prompt).build();
    Ok(OpenRouterModel { agent, config })
}

impl<P: Prompt + Send + Sync> agent_kernel::LanguageModel for OpenRouterModel<P> {
    fn complete(&self, prompt: &str) -> agent_kernel::BoxFuture<'_, Result<String, RunError>> {
        let prompt = prompt.to_owned();
        Box::pin(async move {
            for attempt in 1..=3 {
                match self.agent.prompt(&prompt).await {
                    Ok(content) => {
                        log_telemetry(&self.config, &prompt, &content);
                        return Ok(content);
                    }
                    Err(error) => {
                        let message = error.to_string();
                        if is_retryable_error(&message) && attempt < 3 {
                            let delay =
                                Duration::from_secs(u64::try_from(attempt).unwrap_or(1) * 2);
                            warn!(
                                model = %self.config.model,
                                attempt,
                                delay_secs = delay.as_secs(),
                                error = %message,
                                "OpenRouter request failed with a retryable error"
                            );
                            tokio::time::sleep(delay).await;
                            continue;
                        }

                        return Err(RunError::Provider(message));
                    }
                }
            }

            Err(RunError::Provider(
                "OpenRouter generation exhausted retries".to_owned(),
            ))
        })
    }
}

fn log_telemetry(config: &LlmProviderConfig, input: &str, output: &str) {
    let input_tokens = docx_domain::count_tokens(input);
    let output_tokens = docx_domain::count_tokens(output);

    #[allow(clippy::cast_precision_loss)]
    let total_cost = (input_tokens as f64 * (config.input_cost_per_1m / 1_000_000.0))
        + (output_tokens as f64 * (config.output_cost_per_1m / 1_000_000.0));

    info!(
        model = %config.model,
        input_tokens,
        output_tokens,
        cost_usd = %format!("{total_cost:.6}"),
        "LLM request completed"
    );
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
