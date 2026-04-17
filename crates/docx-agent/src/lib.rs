#![allow(clippy::missing_errors_doc)]

mod config;
mod error;
mod fetch;
mod parser;
mod search;
mod service;

pub use config::{DocxAgentConfig, SearchPolicyConfig};
pub use error::DocxAgentError;
pub use fetch::WebPageFetcher;
pub use parser::DocxDocumentParser;
pub use search::TavilySearchClient;
pub use service::DocxExpansionService;
