use agent_kernel::LanguageModel;
use rig::agent::AgentBuilder;
use rig::providers::openrouter::completion::CompletionModel as OpenRouterCompletionModel;

/// Extension trait for `LanguageModel` that provides integration with the `rig` library.
/// The `Model` associated type allows using different provider models (e.g., OpenAI, Anthropic) 
/// while remaining compatible with rig's type system.
pub trait RigLanguageModel: LanguageModel {
    /// The underlying rig completion model type.
    type Model: rig::completion::CompletionModel;

    /// Returns a rig `AgentBuilder` pre-configured with the model and system prompt.
    fn agent_builder(&self) -> AgentBuilder<Self::Model>;
}

/// A convenience type alias for the default OpenRouter-backed Rig model.
pub type OpenRouterRigModel = dyn RigLanguageModel<Model = OpenRouterCompletionModel>;
