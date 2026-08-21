//! Source code context visualization for error messages
//!
//! This module provides utilities for extracting and pretty-printing source code
//! context around error locations, including line numbers and carets pointing to
//! the error position.

use crate::reader::SourceLoc;

/// Load source code from a file for a given location
///
/// # Arguments
/// * `loc` - The source location
///
/// # Returns
/// The source code contents if the file exists and is readable, None otherwise
pub(crate) fn load_source_for_loc(loc: &SourceLoc) -> Option<String> {
    if loc.is_unknown() || loc.file.starts_with('<') {
        return None;
    }
    std::fs::read_to_string(&loc.file).ok()
}

/// Format source context with line number and caret pointing to error column
///
/// # Arguments
/// * `source` - The complete source code
/// * `location` - The location of the error
///
/// # Returns
/// A formatted string showing the problematic line with a caret, or empty string if location is invalid
///
/// # Example
/// ```text
///  5 | (+ x 1)
///    |    ^
/// ```
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn format_source_context(source: &str, location: &SourceLoc) -> String {
    if location.is_unknown() {
        return String::new();
    }

    match extract_source_line(source, location.line) {
        Some(line) => {
            let mut result = String::new();
            let line_num_str = location.line.to_string();
            let padding = " ".repeat(line_num_str.len());

            result.push_str(&format!("{} | {}\n", line_num_str, line));
            result.push_str(&format!(
                "{} | {}\n",
                padding,
                highlight_column(&line, location.col)
            ));

            result
        }
        None => String::new(),
    }
}

/// Extract a single line from source code by line number (1-based)
///
/// # Arguments
/// * `source` - The complete source code
/// * `line_num` - Line number (1-based)
///
/// # Returns
/// The requested line without trailing newline, or None if line doesn't exist
pub(crate) fn extract_source_line(source: &str, line_num: usize) -> Option<String> {
    if line_num == 0 {
        return None;
    }

    source
        .lines()
        .nth(line_num - 1)
        .map(|line| line.to_string())
}

/// Create a visual caret line pointing to a specific column
///
/// # Arguments
/// * `line` - The source line
/// * `col` - Column number (1-based)
///
/// # Returns
/// A string with spaces and a `^` caret at the appropriate column
///
/// # Example
/// For `col=4` in a line "hello world":
/// Returns `"   ^"` (3 spaces + caret)
pub(crate) fn highlight_column(line: &str, col: usize) -> String {
    if col == 0 {
        return "^".to_string();
    }

    let mut caret = String::new();

    // Count actual display width, accounting for multi-byte characters
    let mut display_width = 0;
    for (_idx, ch) in line.char_indices() {
        if display_width >= col - 1 {
            break;
        }

        // Tab counts as moving to next tab stop (typically 4 or 8 spaces)
        if ch == '\t' {
            display_width += 4; // Use 4 for tab width
        } else {
            display_width += 1;
        }
    }

    // Add spaces for correct column positioning
    caret.push_str(&" ".repeat(display_width.min(col - 1)));
    caret.push('^');

    caret
}

#[cfg(test)]
mod tests;
