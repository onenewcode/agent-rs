#![allow(clippy::missing_errors_doc)]
pub mod agents;
mod model;
mod parser;
mod prompts;

pub use model::{
    BlockKind, Document, DocumentBlock,
    DocxExpandRequest, DocxSourcePolicy,
};

pub use agents::reviewer::ReviewerAgent;
pub use agents::writer::WriterAgent;
pub use parser::DocxDocumentParser;
pub use prompts::count_tokens;
