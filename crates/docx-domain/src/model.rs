use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockKind {
    Heading { level: u8 },
    Paragraph,
    Table,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentBlock {
    pub kind: BlockKind,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub title: Option<String>,
    pub blocks: Vec<DocumentBlock>,
}

impl Document {
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
                BlockKind::Paragraph | BlockKind::Table => {
                    out.push_str(&block.text);
                    out.push_str("\n\n");
                }
            }
        }

        out.trim().to_owned()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocxSourcePolicy {
    pub disable_research: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocxExpandRequest {
    pub document_path: String,
    pub prompt: String,
    #[serde(default)]
    pub user_urls: Vec<String>,
    #[serde(default)]
    pub source_policy: DocxSourcePolicy,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocxDraft {
    pub content: String,
    pub outline: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocxEvaluation {
    pub score: u8,
    pub reason: String,
    pub qualified: bool,
    pub faithfulness_score: u8,
    pub relevance_score: u8,
    pub accuracy_score: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocxAttemptRecord {
    pub attempt: usize,
    pub draft: DocxDraft,
    pub evaluation: DocxEvaluation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocxFinalOutput {
    pub markdown: String,
    pub score: u8,
    pub qualified: bool,
    pub reason: String,
}
