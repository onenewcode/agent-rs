use crate::model::{BlockKind, Document, DocumentBlock};
use agent_kernel::{
    AgentError, DocumentParser, ErrorType, OkOrErr, OrErr, Result, normalize_whitespace,
};
use std::fs::File;
use std::path::Path;

pub struct DocxParser;

impl DocxParser {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn parse(path: &Path) -> Result<Document> {
        let file = File::open(path).or_err(ErrorType::Parse, "failed to open docx file")?;
        let mut archive =
            zip::ZipArchive::new(file).or_err(ErrorType::Parse, "failed to open docx as zip")?;

        let mut content = String::new();
        let mut document_xml = archive
            .by_name("word/document.xml")
            .or_err(ErrorType::Parse, "word/document.xml missing in docx")?;

        std::io::Read::read_to_string(&mut document_xml, &mut content)
            .or_err(ErrorType::Parse, "failed to read word/document.xml")?;

        Self::parse_xml(&content)
    }

    fn parse_xml(xml: &str) -> Result<Document> {
        let doc = roxmltree::Document::parse(xml)
            .or_err(ErrorType::Parse, "failed to parse word/document.xml")?;

        let body = doc
            .descendants()
            .find(|n| n.has_tag_name("body"))
            .or_err(ErrorType::Parse, "document body missing")?;

        let mut blocks = Vec::new();
        for node in body.descendants().filter(|n| n.has_tag_name("p")) {
            let mut p_text = String::new();
            for text_node in node.descendants().filter(|n| n.has_tag_name("t")) {
                if let Some(text) = text_node.text() {
                    p_text.push_str(text);
                }
            }
            let normalized = normalize_whitespace(&p_text);
            if !normalized.is_empty() {
                blocks.push(DocumentBlock {
                    kind: BlockKind::Paragraph,
                    text: normalized,
                });
            }
        }

        if blocks.is_empty() {
            return Err(AgentError::explain(
                ErrorType::Parse,
                "document contains no text paragraphs",
            ));
        }

        Ok(Document {
            title: Some("DOCX Document".to_owned()),
            blocks,
        })
    }
}

impl Default for DocxParser {
    fn default() -> Self {
        Self::new()
    }
}

impl DocumentParser<Document> for DocxParser {
    fn parse_path(&self, path: &Path) -> Result<Document> {
        Self::parse(path)
    }
}
