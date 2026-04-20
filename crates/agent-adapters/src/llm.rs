use agent_kernel::{RunError, TokenUsage, LlmCompletion};
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::providers::openrouter;
use tiktoken_rs::cl100k_base;
use tracing::info;

#[derive(Debug, Clone)]
pub struct LlmProviderConfig {
    pub model: String,
    pub api_key: String,
    pub input_cost_per_1m: f64,
    pub output_cost_per_1m: f64,
}

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
    fn model_id(&self) -> &str {
        &self.config.model
    }

    fn agent_builder(&self) -> rig::agent::AgentBuilder<rig::providers::openrouter::completion::CompletionModel> {
        self.client.agent(&self.config.model).preamble(&self.system_prompt)
    }

    fn complete(&self, prompt: &str) -> agent_kernel::BoxFuture<'_, Result<LlmCompletion, RunError>> {
        let prompt = prompt.to_owned();
        let agent = self.agent_builder().build();
        Box::pin(async move {
            match agent.prompt(&prompt).await {
                Ok(content) => {
                    let prompt_tokens = estimate_tokens(&prompt);
                    let completion_tokens = estimate_tokens(&content);
                    let usage = TokenUsage {
                        prompt_tokens,
                        completion_tokens,
                        total_tokens: prompt_tokens + completion_tokens,
                    };

                    #[allow(clippy::cast_precision_loss)]
                    let estimated_cost_usd = (prompt_tokens as f64 * (self.config.input_cost_per_1m / 1_000_000.0))
                        + (completion_tokens as f64 * (self.config.output_cost_per_1m / 1_000_000.0));

                    log_telemetry(&self.config, prompt_tokens, completion_tokens);
                    Ok(LlmCompletion {
                        text: content,
                        usage,
                        estimated_cost_usd,
                    })
                }
                Err(error) => {
                    let message = error.to_string();
                    let cleaned_message = if message.contains("\n\n\n") {
                        message.lines()
                            .filter(|line| !line.trim().is_empty())
                            .collect::<Vec<_>>()
                            .join(" | ")
                    } else {
                        message
                    };

                    Err(RunError::Provider(cleaned_message))
                }
            }
        })
    }
}

fn log_telemetry(config: &LlmProviderConfig, prompt_tokens: usize, completion_tokens: usize) {
    #[allow(clippy::cast_precision_loss)]
    let total_cost = (prompt_tokens as f64 * (config.input_cost_per_1m / 1_000_000.0))
        + (completion_tokens as f64 * (config.output_cost_per_1m / 1_000_000.0));

    info!(
        model = %config.model,
        prompt_tokens,
        completion_tokens,
        cost_usd = %format!("{total_cost:.6}"),
        "LLM request completed"
    );
}

fn estimate_tokens(text: &str) -> usize {
    cl100k_base().map_or(text.chars().count() / 4, |bpe| {
        bpe.encode_with_special_tokens(text).len()
    })
}
