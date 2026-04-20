use serde::{Deserialize, Serialize};

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
