//! Code formatting support for LSP

use crate::formatter::{format_code, FormatterConfig};
use serde_json::{json, Value};

/// Format an entire document
pub(crate) fn format_document(
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
pub(crate) fn document_end_position(source: &str) -> (u32, u32) {
    let lines: Vec<&str> = source.lines().collect();

    if lines.is_empty() {
        return (0, 0);
    }

    let last_line = (lines.len() - 1) as u32;
    let last_char = lines[lines.len() - 1].len() as u32;

    (last_line, last_char)
}

#[cfg(test)]
mod tests;
