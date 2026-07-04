//! Unit tests (`super` is the parent impl module).

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
