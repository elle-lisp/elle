//! Source code context visualization for error messages
//!
//! This module provides utilities for extracting and pretty-printing source code
//! context around error locations, including line numbers and carets pointing to
//! the error position.

use crate::reader::SourceLoc;

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
pub fn format_source_context(source: &str, location: &SourceLoc) -> String {
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
pub fn extract_source_line(source: &str, line_num: usize) -> Option<String> {
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
pub fn highlight_column(line: &str, col: usize) -> String {
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
mod tests {
    use super::*;

    #[test]
    fn test_extract_source_line() {
        let source = "line 1\nline 2\nline 3";
        assert_eq!(extract_source_line(source, 1), Some("line 1".to_string()));
        assert_eq!(extract_source_line(source, 2), Some("line 2".to_string()));
        assert_eq!(extract_source_line(source, 3), Some("line 3".to_string()));
        assert_eq!(extract_source_line(source, 4), None);
        assert_eq!(extract_source_line(source, 0), None);
    }

    #[test]
    fn test_highlight_column_basic() {
        let line = "(+ x 1)";
        assert_eq!(highlight_column(line, 1), "^");
        assert_eq!(highlight_column(line, 4), "   ^");
        assert_eq!(highlight_column(line, 7), "      ^");
    }

    #[test]
    fn test_highlight_column_out_of_range() {
        let line = "short";
        // Should still produce caret at requested position
        let result = highlight_column(line, 10);
        assert!(result.ends_with('^'));
    }

    #[test]
    fn test_format_source_context() {
        let source = "(var x 1)\n(+ x 2)";
        let loc = SourceLoc::new("test.lisp", 2, 4);

        let result = format_source_context(source, &loc);
        assert!(result.contains("(+ x 2)"));
        assert!(result.contains("^"));
        assert!(result.contains("2 |"));
    }

    #[test]
    fn test_format_source_context_unknown_location() {
        let source = "(var x 1)";
        let loc = SourceLoc::start(); // <unknown> file

        let result = format_source_context(source, &loc);
        assert_eq!(result, "");
    }

    #[test]
    fn test_format_source_context_invalid_line() {
        let source = "line 1";
        let loc = SourceLoc::new("test.lisp", 10, 1);

        let result = format_source_context(source, &loc);
        assert_eq!(result, "");
    }
}
