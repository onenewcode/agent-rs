#![allow(clippy::missing_errors_doc)]

pub mod application;
pub mod config;
pub mod domain;
pub mod error;
pub mod infrastructure;
pub mod steps;

pub use application::DocxExpansionService;
pub use config::{DocxAgentConfig, SearchPolicyConfig};
pub use error::DocxAgentError;
pub use infrastructure::docx::DocxDocumentParser;
pub use infrastructure::fetch::WebPageFetcher;
pub use infrastructure::search::TavilySearchClient;
