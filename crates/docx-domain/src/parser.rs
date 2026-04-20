use std::{fs::File, io::Read, path::Path};

use agent_kernel::{DocumentParser, RunError, normalize_whitespace};
use tracing::{debug, info};

use crate::{BlockKind, Document, DocumentBlock};

const WORD_NAMESPACE: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

#[derive(Debug, Default, Clone, Copy)]
pub struct DocxDocumentParser;

impl DocxDocumentParser {
    pub fn parse(path: &Path) -> Result<Document, RunError> {
        info!(doc = %path.display(), "parsing DOCX document");

        let file = File::open(path).map_err(|error| RunError::Parse(error.to_string()))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|error| RunError::Parse(error.to_string()))?;
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .map_err(|error| RunError::Parse(error.to_string()))?
            .read_to_string(&mut xml)
            .map_err(|error| RunError::Parse(error.to_string()))?;

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

    fn parse_xml(xml: &str) -> Result<Document, RunError> {
        let document =
            roxmltree::Document::parse(xml).map_err(|error| RunError::Parse(error.to_string()))?;
        let mut blocks = Vec::new();

        let body = document
            .descendants()
            .find(|node| is_word_node(node, "body"))
            .ok_or_else(|| RunError::Parse("document body missing".to_owned()))?;

        for node in body.children().filter(roxmltree::Node::is_element) {
            if is_word_node(&node, "p") {
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
            } else if is_word_node(&node, "tbl")
                && let Some(table_markdown) = extract_table_markdown(node)
            {
                blocks.push(DocumentBlock {
                    kind: BlockKind::Table,
                    text: table_markdown,
                });
            }
        }

        if blocks.is_empty() {
            return Err(RunError::Parse(
                "document is empty after parsing".to_owned(),
            ));
        }

        let title = blocks.iter().find_map(|block| match block.kind {
            BlockKind::Heading { .. } => Some(block.text.clone()),
            _ => None,
        });

        Ok(Document { title, blocks })
    }
}

impl DocumentParser<Document> for DocxDocumentParser {
    fn parse_path(&self, path: &Path) -> Result<Document, RunError> {
        Self::parse(path)
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

fn extract_table_markdown(node: roxmltree::Node<'_, '_>) -> Option<String> {
    let mut rows = Vec::new();
    for tr in node.children().filter(|n| is_word_node(n, "tr")) {
        let mut cells = Vec::new();
        for tc in tr.children().filter(|n| is_word_node(n, "tc")) {
            let mut cell_text = String::new();
            for p in tc.children().filter(|n| is_word_node(n, "p")) {
                let p_text = extract_paragraph_text(p);
                if !cell_text.is_empty() && !p_text.is_empty() {
                    cell_text.push(' ');
                }
                cell_text.push_str(&p_text);
            }
            cells.push(cell_text.replace('|', "\\|"));
        }
        if !cells.is_empty() {
            rows.push(cells);
        }
    }

    if rows.is_empty() {
        return None;
    }

    let mut out = String::new();
    let max_cols = rows.iter().map(Vec::len).max().unwrap_or(0);

    for (index, row) in rows.iter().enumerate() {
        out.push_str("| ");
        out.push_str(&row.join(" | "));
        if row.len() < max_cols {
            for _ in 0..(max_cols - row.len()) {
                out.push_str(" | ");
            }
        }
        out.push_str(" |\n");

        if index == 0 {
            out.push_str("| ");
            for _ in 0..max_cols {
                out.push_str("--- | ");
            }
            out.push('\n');
        }
    }

    Some(out.trim().to_owned())
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
    use crate::BlockKind;

    #[test]
    fn parses_headings_and_paragraphs_from_document_xml() -> Result<(), agent_kernel::RunError> {
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
    fn keeps_split_word_runs_contiguous() -> Result<(), agent_kernel::RunError> {
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
