#![allow(clippy::missing_errors_doc)]

mod parser;
mod prompts;

pub use parser::DocxDocumentParser;
pub use prompts::{
    DocxPromptContext, DocxPromptFormatter, DocxPromptTemplates, TokenBudget, count_tokens,
};
