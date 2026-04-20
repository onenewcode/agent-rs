#![allow(clippy::missing_errors_doc)]
pub mod agents;
mod model;
mod parser;
mod prompts;

pub use model::{
    BlockKind, Document, DocumentBlock, DocxAttemptRecord, DocxDraft, DocxEvaluation,
    DocxExpandRequest, DocxFinalOutput, DocxSourcePolicy,
};

pub use agents::reviewer::ReviewerAgent;
pub use agents::writer::WriterAgent;
pub use parser::DocxDocumentParser;
pub use prompts::{
    DocxPromptContext, DocxPromptFormatter, DocxPromptTemplates, TokenBudget, count_tokens,
};
