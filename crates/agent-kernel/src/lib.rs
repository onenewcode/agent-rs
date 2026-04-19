#![allow(clippy::missing_errors_doc)]

use std::{future::Future, path::Path, pin::Pin};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("timeout error: {0}")]
    Timeout(String),
    #[error("evaluation error: {0}")]
    Evaluation(String),
    #[error("internal error: {0}")]
    Internal(String),
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
pub struct Document {
    pub title: Option<String>,
    pub blocks: Vec<DocumentBlock>,
}

impl Document {
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
pub struct SourceMaterial {
    pub kind: SourceKind,
    pub title: Option<String>,
    pub url: String,
    pub summary: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunConstraints {
    pub disable_research: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub prompt: String,
    pub document: Document,
    pub user_urls: Vec<String>,
    pub constraints: RunConstraints,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchMode {
    Disabled,
    Auto,
    Required,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub objective: String,
    pub search_mode: SearchMode,
    pub search_queries: Vec<String>,
    pub evaluation_focus: String,
    pub max_refinement_rounds: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchArtifacts {
    pub queries: Vec<String>,
    pub sources: Vec<SourceMaterial>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub content: String,
    pub outline: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluation {
    pub score: u8,
    pub reason: String,
    pub qualified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageEvent {
    pub stage: String,
    pub attempt: usize,
    pub duration_ms: u128,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptReport {
    pub attempt: usize,
    pub draft: Draft,
    pub evaluation: Evaluation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    pub plan: Plan,
    pub research: ResearchArtifacts,
    pub attempts: Vec<AttemptReport>,
    pub final_output: String,
    pub final_score: u8,
    pub qualified: bool,
    pub final_reason: Option<String>,
    pub stage_events: Vec<StageEvent>,
    pub total_duration_ms: u128,
}

pub trait DocumentParser: Send + Sync {
    fn parse_path(&self, path: &Path) -> Result<Document, RunError>;
}

pub trait LanguageModel: Send + Sync {
    fn complete(&self, prompt: &str) -> BoxFuture<'_, Result<String, RunError>>;
}

pub trait SourceFetcher: Send + Sync {
    fn fetch(&self, url: &str) -> BoxFuture<'_, Result<SourceMaterial, RunError>>;
}

pub trait SearchProvider: Send + Sync {
    fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> BoxFuture<'_, Result<Vec<SourceMaterial>, RunError>>;
}

pub trait Planner: Send + Sync {
    fn plan(&self, task: Task) -> BoxFuture<'_, Result<Plan, RunError>>;
}

pub trait Researcher: Send + Sync {
    fn research(
        &self,
        task: Task,
        plan: Plan,
    ) -> BoxFuture<'_, Result<ResearchArtifacts, RunError>>;
}

pub trait Generator: Send + Sync {
    fn generate(
        &self,
        task: Task,
        plan: Plan,
        research: ResearchArtifacts,
    ) -> BoxFuture<'_, Result<Draft, RunError>>;
}

pub trait Evaluator: Send + Sync {
    fn evaluate(
        &self,
        task: Task,
        plan: Plan,
        research: ResearchArtifacts,
        draft: Draft,
    ) -> BoxFuture<'_, Result<Evaluation, RunError>>;
}

pub trait Refiner: Send + Sync {
    fn refine(
        &self,
        task: Task,
        plan: Plan,
        research: ResearchArtifacts,
        draft: Draft,
        evaluation: Evaluation,
    ) -> BoxFuture<'_, Result<Draft, RunError>>;
}

#[must_use]
pub fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[must_use]
pub fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
