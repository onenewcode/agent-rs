use agent_kernel::{AgentError, ErrorType, LanguageModel, LlmCompletion, Result, RetryType};
use agent_rig::RigLanguageModel;
use rig::agent::AgentBuilder;
use rig::completion::Prompt;
use rig::client::CompletionClient;
use rig::providers::openrouter::completion::CompletionModel as OpenRouterCompletionModel;

pub struct OpenRouterModel {
    model_id: String,
    client: rig::providers::openrouter::Client,
}

impl OpenRouterModel {
    /// Creates a new `OpenRouterModel`.
    pub fn new(model_id: String, api_key: &str) -> Result<Self> {
        let client = rig::providers::openrouter::Client::new(api_key).map_err(|e| {
            AgentError::explain(
                ErrorType::Config,
                format!("failed to create OpenRouter client: {e}"),
            )
        })?;
        Ok(Self { model_id, client })
    }

    #[must_use]
    pub fn from_client(model_id: String, client: rig::providers::openrouter::Client) -> Self {
        Self { model_id, client }
    }
}

impl LanguageModel for OpenRouterModel {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn complete(&self, prompt: &str) -> agent_kernel::BoxFuture<'_, Result<LlmCompletion>> {
        let prompt = prompt.to_owned();
        let model_id = self.model_id.clone();
        let client = self.client.clone();
        Box::pin(async move {
            let agent = client.agent(&model_id).build();
            let text = agent.prompt(&prompt).await.map_err(|error| {
                let msg = error.to_string();
                let mut err = AgentError::explain(ErrorType::Provider, msg.clone());

                if msg.contains("429")
                    || msg.contains("rate limit")
                    || msg.contains("timeout")
                    || msg.contains("500")
                    || msg.contains("502")
                    || msg.contains("503")
                    || msg.contains("504")
                {
                    err = err.set_retry(RetryType::Retry);
                }

                err
            })?;

            // Accurate Token Counting using centralized utility
            let usage = agent_kernel::TokenEstimator::estimate(&prompt, &text);

            let mut telemetry = agent_kernel::Telemetry::default();
            telemetry.add_usage(&model_id, usage);

            Ok(LlmCompletion {
                text,
                usage,
                estimated_cost_usd: telemetry.estimated_cost_usd,
            })
        })
    }
}

impl RigLanguageModel for OpenRouterModel {
    type Model = OpenRouterCompletionModel;

    fn agent_builder(&self) -> AgentBuilder<Self::Model> {
        self.client.agent(&self.model_id)
    }
}
