use agent_kernel::{
    Error, ErrorSource, ErrorType, Result, TokenUsage, LlmCompletion, LanguageModel, RetryType,
};
use rig::providers::openrouter::completion::CompletionModel as OpenRouterCompletionModel;
use rig::agent::AgentBuilder;
use rig::client::completion::CompletionClient;
use rig::completion::Prompt;

pub struct OpenRouterModel {
    model_id: String,
    client: rig::providers::openrouter::Client,
}

impl OpenRouterModel {
    #[must_use]
    pub fn new(model_id: String, api_key: String) -> Self {
        let client = rig::providers::openrouter::Client::new(&api_key).expect("failed to create OpenRouter client");
        Self { model_id, client }
    }

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
            let text = agent
                .prompt(&prompt)
                .await
                .map_err(|error| {
                    let msg = error.to_string();
                    let mut err = Error::explain(ErrorType::Provider, msg.clone());
                    
                    if msg.contains("429") || msg.contains("rate limit") || msg.contains("timeout") || msg.contains("500") || msg.contains("502") || msg.contains("503") || msg.contains("504") {
                        err = err.set_retry(RetryType::Retry);
                    }
                    
                    Box::new(err.set_source(ErrorSource::Upstream))
                })?;

            Ok(LlmCompletion {
                text,
                usage: TokenUsage::default(),
                estimated_cost_usd: 0.0,
            })
        })
    }

    fn agent_builder(
        &self,
    ) -> AgentBuilder<OpenRouterCompletionModel> {
        self.client.agent(&self.model_id)
    }
}
