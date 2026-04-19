use std::time::Duration;

use rig::{client::CompletionClient, completion::Prompt, providers::openrouter};
use tracing::{info, warn};

use crate::error::DocxAgentError;
use agent_core::config::LlmConfig;

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

pub(crate) struct RigLlmBackend<P: Prompt> {
    agent: P,
    config: LlmConfig,
    max_attempts: usize,
}

impl<P: Prompt> agent_core::LlmBackend for RigLlmBackend<P> {
    fn prompt(
        &self,
        prompt: &str,
    ) -> agent_core::BoxFuture<'_, Result<String, agent_core::ExpansionError>> {
        let prompt = prompt.to_owned();
        Box::pin(async move {
            generate_with_retry(&self.agent, &prompt, &self.config, self.max_attempts)
                .await
                .map_err(|e| match e {
                    DocxAgentError::Agent(inner) => inner,
                    _ => agent_core::ExpansionError::Internal(e.to_string()),
                })
        })
    }
}

#[allow(clippy::needless_pass_by_value)]
pub(crate) fn build_agent(
    http: reqwest::Client,
    config: LlmConfig,
    system_prompt: String,
    max_attempts: usize,
) -> Result<RigLlmBackend<impl Prompt>, DocxAgentError> {
    let client = openrouter::Client::builder()
        .api_key(config.api_key.as_str())
        .http_client(http)
        .build()
        .map_err(|error| {
            DocxAgentError::Agent(agent_core::ExpansionError::Provider(format!(
                "failed to build OpenRouter client: {error}"
            )))
        })?;

    let agent = client.agent(&config.model).preamble(&system_prompt).build();

    Ok(RigLlmBackend {
        agent,
        config,
        max_attempts,
    })
}

pub(crate) async fn generate_with_retry(
    agent: &impl Prompt,
    prompt: &str,
    config: &LlmConfig,
    max_attempts: usize,
) -> Result<String, DocxAgentError> {
    for attempt in 1..=max_attempts {
        match agent.prompt(prompt).await {
            Ok(content) => {
                log_telemetry(config, prompt, &content);
                return Ok(content);
            }
            Err(error) => {
                let message = error.to_string();
                if is_retryable_error(&message) && attempt < max_attempts {
                    let delay = Duration::from_secs(attempt as u64 * 2);
                    warn!(
                        model = %config.model,
                        attempt,
                        delay_secs = delay.as_secs(),
                        error = %message,
                        "OpenRouter request failed with a retryable error"
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }

                return Err(DocxAgentError::Agent(agent_core::ExpansionError::Provider(
                    message,
                )));
            }
        }
    }

    Err(DocxAgentError::Agent(agent_core::ExpansionError::Provider(
        "OpenRouter generation exhausted retries".to_owned(),
    )))
}

fn log_telemetry(config: &LlmConfig, input: &str, output: &str) {
    let input_tokens = crate::domain::count_tokens(input);
    let output_tokens = crate::domain::count_tokens(output);

    let input_cost_per_token = config.input_cost_per_1m / 1_000_000.0;
    let output_cost_per_token = config.output_cost_per_1m / 1_000_000.0;

    #[allow(clippy::cast_precision_loss)]
    let total_cost = (input_tokens as f64 * input_cost_per_token)
        + (output_tokens as f64 * output_cost_per_token);

    info!(
        model = %config.model,
        input_tokens,
        output_tokens,
        cost_usd = %format!("{:.6}", total_cost),
        "LLM request completed"
    );
}

pub(crate) async fn generate_optimized_search_query(
    agent: &(impl agent_core::LlmBackend + ?Sized),
    title: Option<&str>,
    prompt: &str,
    model_name: &str,
) -> Result<String, DocxAgentError> {
    let title_context = title.map_or(String::new(), |t| format!("Document title: {t}\n"));
    let generation_prompt = format!(
        "{title_context}User intent: {prompt}\n\nBased on the document title and user intent, generate ONE concise, effective search query to find supporting materials. Output ONLY the query text without quotes or explanations.",
    );

    info!(model = %model_name, "generating optimized search query via LLM");

    let query = agent
        .prompt(&generation_prompt)
        .await
        .map_err(DocxAgentError::Agent)?;
    let trimmed = query.trim().trim_matches('"').to_owned();

    info!(query = %trimmed, "LLM generated optimized search query");
    Ok(trimmed)
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
