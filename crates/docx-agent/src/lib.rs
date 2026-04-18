#![allow(clippy::missing_errors_doc)]

mod application;
mod config;
mod domain;
mod error;
mod infrastructure;

pub use application::DocxExpansionService;
pub use config::{DocxAgentConfig, SearchPolicyConfig};
pub use error::DocxAgentError;
pub use infrastructure::docx::DocxDocumentParser;
pub use infrastructure::fetch::WebPageFetcher;
pub use infrastructure::search::TavilySearchClient;
