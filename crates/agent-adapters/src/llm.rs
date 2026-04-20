use std::time::Duration;

use agent_kernel::RunError;
use rig::{client::CompletionClient, completion::Prompt, providers::openrouter};
use tiktoken_rs::cl100k_base;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct LlmProviderConfig {
    pub model: String,
    pub api_key: String,
    pub input_cost_per_1m: f64,
    pub output_cost_per_1m: f64,
}

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

pub struct OpenRouterModel {
    client: openrouter::Client,
    config: LlmProviderConfig,
    system_prompt: String,
}

#[allow(clippy::needless_pass_by_value)]
pub fn build_openrouter_model(
    http: reqwest::Client,
    config: LlmProviderConfig,
    system_prompt: String,
) -> Result<OpenRouterModel, RunError> {
    let client = openrouter::Client::builder()
        .api_key(config.api_key.as_str())
        .http_client(http)
        .build()
        .map_err(|error| {
            RunError::Provider(format!("failed to build OpenRouter client: {error}"))
        })?;
    Ok(OpenRouterModel { client, config, system_prompt })
}

impl agent_kernel::LanguageModel for OpenRouterModel {
    fn agent_builder(&self) -> rig::agent::AgentBuilder<rig::providers::openrouter::completion::CompletionModel> {
        self.client.agent(&self.config.model).preamble(&self.system_prompt)
    }

    fn complete<'a>(&'a self, context: &'a mut agent_kernel::WorkflowContext, prompt: &str) -> agent_kernel::BoxFuture<'a, Result<String, RunError>> {
        let prompt = prompt.to_owned();
        let agent = self.agent_builder().build();
        Box::pin(async move {
            for attempt in 1..=3 {
                match agent.prompt(&prompt).await {
                    Ok(content) => {
                        let prompt_tokens = estimate_tokens(&prompt);
                        let completion_tokens = estimate_tokens(&content);

                        if let Ok(telemetry) = context.state_mut::<agent_kernel::Telemetry>() {
                            telemetry.usage.prompt_tokens += prompt_tokens;
                            telemetry.usage.completion_tokens += completion_tokens;
                            telemetry.usage.total_tokens += prompt_tokens + completion_tokens;

                            let cost = (prompt_tokens as f64 * (self.config.input_cost_per_1m / 1_000_000.0))
                                + (completion_tokens as f64 * (self.config.output_cost_per_1m / 1_000_000.0));
                            telemetry.estimated_cost_usd += cost;
                        }

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
    let input_units = input.chars().count();
    let output_units = output.chars().count();

    #[allow(clippy::cast_precision_loss)]
    let total_cost = (input_units as f64 * (config.input_cost_per_1m / 1_000_000.0))
        + (output_units as f64 * (config.output_cost_per_1m / 1_000_000.0));

    info!(
        model = %config.model,
        input_units,
        output_units,
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

fn estimate_tokens(text: &str) -> usize {
    cl100k_base().map(|bpe| bpe.encode_with_special_tokens(text).len()).unwrap_or(text.chars().count() / 4)
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
