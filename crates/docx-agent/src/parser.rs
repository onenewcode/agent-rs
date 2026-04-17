use std::{fs::File, io::Read, path::Path};

use agent_core::{normalize_whitespace, BlockKind, DocumentBlock, DocumentParser, ParsedDocument};
use tracing::{debug, info};

use crate::error::DocxAgentError;

const WORD_NAMESPACE: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[derive(Debug, Default, Clone, Copy)]
pub struct DocxDocumentParser;

impl DocxDocumentParser {
    pub fn parse(path: &Path) -> Result<ParsedDocument, DocxAgentError> {
        info!(doc = %path.display(), "parsing DOCX document");

        let file = File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")?
            .read_to_string(&mut xml)?;

        let parsed = Self::parse_xml(&xml)?;
        info!(
            doc = %path.display(),
            title = parsed.title.as_deref().unwrap_or(""),
            blocks = parsed.blocks.len(),
            "parsed DOCX document"
        );
        debug!(doc = %path.display(), xml_chars = xml.chars().count(), "loaded document xml");
        Ok(parsed)
    }

    fn parse_xml(xml: &str) -> Result<ParsedDocument, DocxAgentError> {
        let document = roxmltree::Document::parse(xml)?;
        let mut blocks = Vec::new();

        for node in document
            .descendants()
            .filter(|node| is_word_node(node, "p"))
        {
            let text = extract_paragraph_text(node);
            if text.is_empty() {
                continue;
            }

            let block_kind = paragraph_style(node)
                .and_then(|style| heading_level(&style))
                .map_or(BlockKind::Paragraph, |level| BlockKind::Heading { level });

            blocks.push(DocumentBlock {
                kind: block_kind,
                text,
            });
        }

        if blocks.is_empty() {
            return Err(DocxAgentError::EmptyDocument);
        }

        let title = blocks.iter().find_map(|block| match block.kind {
            BlockKind::Heading { .. } => Some(block.text.clone()),
            BlockKind::Paragraph => None,
        });

        Ok(ParsedDocument { title, blocks })
    }
}

impl DocumentParser for DocxDocumentParser {
    fn parse_path(&self, path: &Path) -> Result<ParsedDocument, agent_core::BoxError> {
        Self::parse(path).map_err(Into::into)
    }
}

fn is_word_node(node: &roxmltree::Node<'_, '_>, tag_name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == tag_name
        && node.tag_name().namespace() == Some(WORD_NAMESPACE)
}

fn paragraph_style(node: roxmltree::Node<'_, '_>) -> Option<String> {
    node.children()
        .find(|child| is_word_node(child, "pPr"))
        .and_then(|properties| {
            properties
                .children()
                .find(|child| is_word_node(child, "pStyle"))
                .and_then(|style| {
                    style
                        .attributes()
                        .find(|attribute| attribute.name() == "val")
                        .map(|attribute| attribute.value().to_owned())
                })
        })
}

fn extract_paragraph_text(node: roxmltree::Node<'_, '_>) -> String {
    let mut merged = String::new();

    for text_node in node.descendants().filter(|child| is_word_node(child, "t")) {
        if let Some(text) = text_node.text() {
            merged.push_str(text);
        }
    }

    normalize_whitespace(&merged)
}

fn heading_level(style: &str) -> Option<u8> {
    let lower = style.to_ascii_lowercase();
    if !lower.starts_with("heading") {
        return None;
    }

    lower
        .trim_start_matches("heading")
        .parse::<u8>()
        .ok()
        .map(|level| level.clamp(1, 6))
        .or(Some(1))
}

#[cfg(test)]
mod tests {
    use super::DocxDocumentParser;
    use crate::error::DocxAgentError;
    use agent_core::BlockKind;

    #[test]
    fn parses_headings_and_paragraphs_from_document_xml() -> Result<(), DocxAgentError> {
        let xml = r#"
            <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
              <w:body>
                <w:p>
                  <w:pPr><w:pStyle w:val="Heading1" /></w:pPr>
                  <w:r><w:t>Project Overview</w:t></w:r>
                </w:p>
                <w:p>
                  <w:r><w:t>First paragraph.</w:t></w:r>
                  <w:r><w:t xml:space="preserve"> More text.</w:t></w:r>
                </w:p>
              </w:body>
            </w:document>
        "#;

        let parsed = DocxDocumentParser::parse_xml(xml)?;
        assert_eq!(parsed.title.as_deref(), Some("Project Overview"));
        assert_eq!(parsed.blocks.len(), 2);
        assert_eq!(parsed.blocks[0].kind, BlockKind::Heading { level: 1 });
        assert_eq!(parsed.blocks[1].text, "First paragraph. More text.");
        Ok(())
    }

    #[test]
    fn keeps_split_word_runs_contiguous() -> Result<(), DocxAgentError> {
        let xml = r#"
            <w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
              <w:body>
                <w:p>
                  <w:r><w:t>exam</w:t></w:r>
                  <w:r><w:t>ple</w:t></w:r>
                </w:p>
              </w:body>
            </w:document>
        "#;

        let parsed = DocxDocumentParser::parse_xml(xml)?;
        assert_eq!(parsed.blocks.len(), 1);
        assert_eq!(parsed.blocks[0].text, "example");
        Ok(())
    }
}
