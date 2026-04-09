#![allow(clippy::missing_errors_doc)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockKind {
    Heading { level: u8 },
    Paragraph,
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
                BlockKind::Paragraph => {
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
}

pub trait DocumentParser: Send + Sync {
    fn parse_path(&self, path: &std::path::Path) -> Result<ParsedDocument, BoxError>;
}

#[async_trait]
pub trait SearchBackend: Send + Sync {
    async fn search(&self, query: &str, max_results: usize)
    -> Result<Vec<FetchedSource>, BoxError>;
}

#[async_trait]
pub trait UrlFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<FetchedSource, BoxError>;
}

#[async_trait]
pub trait ExpansionRuntime: Send + Sync {
    async fn expand(&self, request: ExpansionRequest) -> Result<ExpansionResult, BoxError>;
}
