//! Code formatting support for LSP

use crate::formatter::{format_code, FormatterConfig};
use serde_json::{json, Value};

/// Result of a formatting operation
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

/// A range in a document
#[derive(Debug, Clone)]
pub struct Range {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// Format an entire document
pub fn format_document(
    source: &str,
    end_line: u32,
    end_character: u32,
) -> Result<Vec<Value>, String> {
    let config = FormatterConfig::default();

    let formatted = format_code(source, &config)?;

    let edit = json!({
        "range": {
            "start": { "line": 0, "character": 0 },
            "end": { "line": end_line, "character": end_character }
        },
        "newText": formatted
    });

    Ok(vec![edit])
}

/// Calculate the line and character position at the end of a document
pub fn document_end_position(source: &str) -> (u32, u32) {
    let lines: Vec<&str> = source.lines().collect();

    if lines.is_empty() {
        return (0, 0);
    }

    let last_line = (lines.len() - 1) as u32;
    let last_char = lines[lines.len() - 1].len() as u32;

    (last_line, last_char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_end_position_empty() {
        let (line, char) = document_end_position("");
        assert_eq!(line, 0);
        assert_eq!(char, 0);
    }

    #[test]
    fn test_document_end_position_single_line() {
        let (line, char) = document_end_position("hello");
        assert_eq!(line, 0);
        assert_eq!(char, 5);
    }

    #[test]
    fn test_document_end_position_multiple_lines() {
        let (line, char) = document_end_position("hello\nworld");
        assert_eq!(line, 1);
        assert_eq!(char, 5);
    }

    #[test]
    fn test_format_document_simple() {
        let source = "42";
        let (end_line, end_char) = document_end_position(source);
        let result = format_document(source, end_line, end_char);

        assert!(result.is_ok());
        let edits = result.unwrap();
        assert_eq!(edits.len(), 1);

        let edit = &edits[0];
        assert!(edit.get("range").is_some());
        assert!(edit.get("newText").is_some());
    }
}
