#![allow(clippy::missing_errors_doc)]

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    future::Future,
    path::Path,
    pin::Pin,
    sync::Arc,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
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
    #[error("workflow error: {0}")]
    Workflow(String),
    #[error("artifact error: {0}")]
    Artifact(String),
    #[error("internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOptions {
    pub global_timeout_secs: u64,
    pub capture_artifacts: bool,
}

impl RunOptions {
    #[must_use]
    pub fn with_defaults(global_timeout_secs: u64, capture_artifacts: bool) -> Self {
        Self {
            global_timeout_secs,
            capture_artifacts,
        }
    }
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            global_timeout_secs: 180,
            capture_artifacts: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRequest {
    pub workflow: String,
    pub input: Value,
    #[serde(default)]
    pub options: RunOptions,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualityGate {
    pub score: u8,
    pub passed: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactEnvelope {
    pub key: String,
    pub kind: String,
    pub value: Value,
}

// Typed State implementation
#[derive(Default)]
pub struct TypeMap {
    data: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl TypeMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<T: Send + Sync + 'static>(&mut self, val: T) {
        self.data.insert(TypeId::of::<T>(), Box::new(val));
    }

    #[must_use]
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.data.get(&TypeId::of::<T>()).and_then(|b| b.downcast_ref())
    }

    #[must_use]
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.data.get_mut(&TypeId::of::<T>()).and_then(|b| b.downcast_mut())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventStatus {
    Started,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEvent {
    pub step_id: String,
    pub status: EventStatus,
    pub duration_ms: u128,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    pub run_id: String,
    pub workflow: String,
    pub qualified: bool,
    pub output_artifact: Option<ArtifactEnvelope>,
    pub artifacts: Vec<ArtifactEnvelope>,
    pub events: Vec<RunEvent>,
    pub total_duration_ms: u128,
}

impl RunReport {
    pub fn output<T: DeserializeOwned>(&self) -> Result<T, RunError> {
        let artifact = self.output_artifact.as_ref().ok_or_else(|| {
            RunError::Artifact("workflow completed without an output artifact".to_owned())
        })?;
        serde_json::from_value(artifact.value.clone()).map_err(|error| {
            RunError::Artifact(format!(
                "failed to decode output artifact `{}`: {error}",
                artifact.key
            ))
        })
    }
}

pub trait DocumentParser<T>: Send + Sync {
    fn parse_path(&self, path: &Path) -> Result<T, RunError>;
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

pub trait ArtifactStore: Send + Sync {
    fn persist(&self, report: &RunReport) -> BoxFuture<'_, Result<(), RunError>>;
}

pub trait CapabilityRegistry: Send + Sync {
    fn llm(&self, name: &str) -> Result<Arc<dyn LanguageModel>, RunError>;
    fn source_fetcher(&self) -> Result<Arc<dyn SourceFetcher>, RunError>;
    fn search_provider(&self) -> Option<Arc<dyn SearchProvider>>;
    fn artifact_store(&self) -> Option<Arc<dyn ArtifactStore>>;
}

pub struct WorkflowContext {
    pub run_id: String,
    pub workflow: String,
    pub input: Value,
    pub state: TypeMap,
    pub artifacts: Vec<ArtifactEnvelope>,
    pub services: Arc<dyn CapabilityRegistry>,
}

impl WorkflowContext {
    #[must_use]
    pub fn new(
        run_id: String,
        workflow: String,
        input: Value,
        services: Arc<dyn CapabilityRegistry>,
    ) -> Self {
        Self {
            run_id,
            workflow,
            input,
            state: TypeMap::new(),
            artifacts: Vec::new(),
            services,
        }
    }

    pub fn input_as<T: DeserializeOwned>(&self) -> Result<T, RunError> {
        serde_json::from_value(self.input.clone()).map_err(|error| {
            RunError::Workflow(format!("failed to decode workflow input: {error}"))
        })
    }

    pub fn emit_artifact<T: Serialize>(
        &mut self,
        key: impl Into<String>,
        kind: impl Into<String>,
        value: &T,
    ) -> Result<(), RunError> {
        let key = key.into();
        let kind = kind.into();
        let serialized = serde_json::to_value(value)
            .map_err(|error| RunError::Artifact(format!("failed to serialize `{key}`: {error}")))?;
        self.artifacts.push(ArtifactEnvelope {
            key,
            kind,
            value: serialized,
        });
        Ok(())
    }

    pub fn state<T: 'static>(&self) -> Result<&T, RunError> {
        self.state.get::<T>().ok_or_else(|| {
            RunError::Artifact(format!(
                "missing state of type `{}`",
                std::any::type_name::<T>()
            ))
        })
    }

    pub fn state_mut<T: 'static>(&mut self) -> Result<&mut T, RunError> {
        self.state.get_mut::<T>().ok_or_else(|| {
            RunError::Artifact(format!(
                "missing state of type `{}`",
                std::any::type_name::<T>()
            ))
        })
    }

    pub fn insert_state<T: Send + Sync + 'static>(&mut self, value: T) {
        self.state.insert(value);
    }
}

pub enum StepTransition {
    Next(&'static str),
    Complete {
        output_artifact: Option<ArtifactEnvelope>,
        qualified: bool,
    },
}

pub struct StepExecution {
    pub context: WorkflowContext,
    pub transition: StepTransition,
}

impl StepExecution {
    #[must_use]
    pub fn next(context: WorkflowContext, next_step: &'static str) -> Self {
        Self {
            context,
            transition: StepTransition::Next(next_step),
        }
    }

    #[must_use]
    pub fn complete(context: WorkflowContext, output: Option<ArtifactEnvelope>, qualified: bool) -> Self {
        Self {
            context,
            transition: StepTransition::Complete {
                output_artifact: output,
                qualified,
            },
        }
    }
}

pub trait WorkflowStep: Send + Sync {
    fn id(&self) -> &'static str;
    fn execute(
        &self,
        context: WorkflowContext,
    ) -> BoxFuture<'static, Result<StepExecution, RunError>>;
}

pub struct WorkflowDefinition {
    pub workflow_id: &'static str,
    pub initial_step: &'static str,
    pub steps: HashMap<&'static str, Arc<dyn WorkflowStep>>,
}

impl WorkflowDefinition {
    #[must_use]
    pub fn new(workflow_id: &'static str, initial_step: &'static str, steps: Vec<Arc<dyn WorkflowStep>>) -> Self {
        let steps_map = steps.into_iter().map(|s| (s.id(), s)).collect();
        Self {
            workflow_id,
            initial_step,
            steps: steps_map,
        }
    }
}

pub trait Workflow: Send + Sync {
    fn id(&self) -> &'static str;
    fn build(&self, request: &RunRequest) -> Result<WorkflowDefinition, RunError>;
}

#[must_use]
pub fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[must_use]
pub fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
