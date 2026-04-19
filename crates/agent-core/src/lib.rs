#![allow(clippy::missing_errors_doc)]

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod config;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum ExpansionError {
    #[error("Research error ({kind}): {message}")]
    Research { kind: String, message: String },
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Rate limited: {0}")]
    RateLimited(String),
    #[error("Provider error: {0}")]
    Provider(String),
    #[error("Timeout error: {0}")]
    Timeout(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Evaluation error: {0}")]
    Evaluation(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockKind {
    Heading { level: u8 },
    Paragraph,
    Table,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentBlock {
    pub kind: BlockKind,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedDocument {
    pub title: Option<String>,
    pub blocks: Vec<DocumentBlock>,
}

impl ParsedDocument {
    #[must_use]
    pub fn render_markdown(&self) -> String {
        let mut out = String::new();

        for block in &self.blocks {
            match &block.kind {
                BlockKind::Heading { level } => {
                    let heading_level = usize::from(*level).clamp(1, 6);
                    out.push_str(&"#".repeat(heading_level));
                    out.push(' ');
                    out.push_str(&block.text);
                    out.push_str("\n\n");
                }
                BlockKind::Paragraph | BlockKind::Table => {
                    out.push_str(&block.text);
                    out.push_str("\n\n");
                }
            }
        }

        out.trim().to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceKind {
    UserUrl,
    SearchResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchedSource {
    pub kind: SourceKind,
    pub title: Option<String>,
    pub url: String,
    pub summary: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationResult {
    pub score: u8,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluationRequest {
    pub prompt: String,
    pub content: String,
    pub sources: Vec<FetchedSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpansionRequest {
    pub prompt: String,
    pub document: ParsedDocument,
    pub user_urls: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpansionResult {
    pub content: String,
    pub search_queries: Vec<String>,
    pub sources: Vec<FetchedSource>,
    pub score: u8,
    pub is_qualified: bool,
    pub evaluation_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchResult {
    pub search_queries: Vec<String>,
    pub sources: Vec<FetchedSource>,
}

pub trait LlmBackend: Send + Sync {
    fn prompt(&self, prompt: &str) -> BoxFuture<'_, Result<String, ExpansionError>>;
}

pub trait DocumentParser: Send + Sync {
    fn parse_path(&self, path: &std::path::Path) -> Result<ParsedDocument, ExpansionError>;
}

pub trait SearchBackend: Send + Sync {
    fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> BoxFuture<'_, Result<Vec<FetchedSource>, ExpansionError>>;
}

pub trait UrlFetcher: Send + Sync {
    fn fetch(&self, url: &str) -> BoxFuture<'_, Result<FetchedSource, ExpansionError>>;
}

pub trait EvaluatorRuntime: Send + Sync {
    fn evaluate(
        &self,
        request: EvaluationRequest,
    ) -> BoxFuture<'_, Result<EvaluationResult, ExpansionError>>;
}

pub trait ResearchRuntime: Send + Sync {
    fn research(
        &self,
        request: ExpansionRequest,
    ) -> BoxFuture<'_, Result<ResearchResult, ExpansionError>>;
}

#[must_use]
pub fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[must_use]
pub fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub trait ExpansionRuntime: Send + Sync {
    fn expand(
        &self,
        request: ExpansionRequest,
    ) -> BoxFuture<'_, Result<ExpansionResult, ExpansionError>>;

    fn generate(
        &self,
        request: ExpansionRequest,
        research: ResearchResult,
    ) -> BoxFuture<'_, Result<ExpansionResult, ExpansionError>>;
}

pub trait Step: Send + Sync {
    fn name(&self) -> &str;
    fn execute<'a>(
        &self,
        request: &'a mut ExpansionRequest,
        current_result: Option<ExpansionResult>,
        research: Option<ResearchResult>,
    ) -> BoxFuture<'a, Result<(Option<ExpansionResult>, Option<ResearchResult>), ExpansionError>>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PipelineState {
    current_result: Option<ExpansionResult>,
    current_research: Option<ResearchResult>,
}

impl PipelineState {
    #[must_use]
    pub fn new(
        current_result: Option<ExpansionResult>,
        current_research: Option<ResearchResult>,
    ) -> Self {
        Self {
            current_result,
            current_research,
        }
    }

    #[must_use]
    pub fn result(&self) -> Option<&ExpansionResult> {
        self.current_result.as_ref()
    }

    #[must_use]
    pub fn research(&self) -> Option<&ResearchResult> {
        self.current_research.as_ref()
    }

    #[must_use]
    pub fn into_retry_state(self) -> Self {
        Self {
            current_result: None,
            current_research: self.current_research,
        }
    }

    pub fn into_result(self) -> Result<ExpansionResult, ExpansionError> {
        self.current_result.ok_or_else(|| {
            ExpansionError::Internal("Pipeline finished without a result".to_owned())
        })
    }
}

pub struct Pipeline {
    pub steps: Vec<Box<dyn Step>>,
}

impl Pipeline {
    #[must_use]
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    pub fn add_step(&mut self, step: Box<dyn Step>) {
        self.steps.push(step);
    }

    pub async fn run(
        &self,
        request: &mut ExpansionRequest,
    ) -> Result<ExpansionResult, ExpansionError> {
        self.run_with_state(request, PipelineState::default())
            .await?
            .into_result()
    }

    pub async fn run_with_state(
        &self,
        request: &mut ExpansionRequest,
        mut state: PipelineState,
    ) -> Result<PipelineState, ExpansionError> {
        for step in &self.steps {
            let (next_result, next_research) = step
                .execute(
                    request,
                    state.current_result.take(),
                    state.current_research.take(),
                )
                .await?;
            state.current_result = next_result;
            state.current_research = next_research;
        }

        Ok(state)
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new()
    }
}
