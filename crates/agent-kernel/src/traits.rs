use crate::error::Result;
use crate::telemetry::TokenUsage;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait DocumentParser<T>: Send + Sync {
    fn parse_path(&self, path: &Path) -> Result<T>;
}

pub struct LlmCompletion {
    pub text: String,
    pub usage: TokenUsage,
    pub estimated_cost_usd: f64,
}

pub trait LanguageModel: Send + Sync {
    /// Returns the ID of the model.
    fn model_id(&self) -> &str;

    /// Completes the prompt and returns the text, token usage, and cost.
    fn complete(&self, prompt: &str) -> BoxFuture<'_, Result<LlmCompletion>>;

    /// Returns a rig `AgentBuilder` pre-configured with the model and system prompt.
    fn agent_builder(
        &self,
    ) -> rig::agent::AgentBuilder<rig::providers::openrouter::completion::CompletionModel>;
}

pub trait AutonomousAgent: Send + Sync {
    fn role(&self) -> &'static str;
    fn run<'a>(&'a self, session: &'a crate::agent::AgentSession) -> BoxFuture<'a, Result<()>>;
}

pub trait AgentAuditor: Send + Sync {
    fn audit_turn<'a>(
        &'a self,
        session: &'a crate::agent::AgentSession,
    ) -> BoxFuture<'a, Result<crate::agent::AuditVerdict>>;

    fn generate_final_report(
        &self,
        context: &crate::agent::AgentContext,
    ) -> crate::agent::AuditReport;
}

pub trait SourceFetcher: Send + Sync {
    fn fetch(&self, url: &str) -> BoxFuture<'_, Result<crate::SourceMaterial>>;
}

pub trait SearchProvider: Send + Sync {
    fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> BoxFuture<'_, Result<Vec<crate::SourceMaterial>>>;
}

pub trait ArtifactStore: Send + Sync {
    fn persist(&self, report: &crate::artifact::RunReport) -> BoxFuture<'_, Result<()>>;
}
