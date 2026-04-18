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
                log_telemetry(model, prompt, &content);
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

fn log_telemetry(model: &str, input: &str, output: &str) {
    let input_tokens = crate::domain::count_tokens(input);
    let output_tokens = crate::domain::count_tokens(output);

    // GPT-4o-mini pricing as baseline
    let input_cost_per_token = 0.15 / 1_000_000.0;
    let output_cost_per_token = 0.60 / 1_000_000.0;

    let total_cost =
        (input_tokens as f64 * input_cost_per_token) + (output_tokens as f64 * output_cost_per_token);

    info!(
        model,
        input_tokens,
        output_tokens,
        cost_usd = %format!("{:.6}", total_cost),
        "LLM request completed"
    );
}

pub(crate) async fn generate_optimized_search_query(
    agent: &impl Prompt,
    title: Option<&str>,
    prompt: &str,
    model: &str,
) -> Result<String, DocxAgentError> {
    let title_context = title.map_or("".to_owned(), |t| format!("Document title: {t}\n"));
    let generation_prompt = format!(
        "{title_context}User intent: {prompt}\n\nBased on the document title and user intent, generate ONE concise, effective search query to find supporting materials. Output ONLY the query text without quotes or explanations.",
    );

    info!(model, "generating optimized search query via LLM");

    let query = generate_with_retry(agent, &generation_prompt, model, 1).await?;
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
