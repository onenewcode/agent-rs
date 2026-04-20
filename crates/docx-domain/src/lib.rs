#![allow(clippy::missing_errors_doc)]

mod model;
mod parser;
mod prompts;
mod workflow;

pub use model::{
    BlockKind, Document, DocumentBlock, DocxAttemptRecord, DocxDraft, DocxEvaluation,
    DocxExpandRequest, DocxFinalOutput, DocxPlan, DocxResearchArtifacts, DocxSourcePolicy,
};
pub use parser::DocxDocumentParser;
pub use prompts::{
    DocxPromptContext, DocxPromptFormatter, DocxPromptTemplates, TokenBudget, count_tokens,
};
pub use workflow::{DocxWorkflow, DocxWorkflowConfig};
