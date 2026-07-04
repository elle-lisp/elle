//! Unit tests (`super` is the parent impl module).

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
