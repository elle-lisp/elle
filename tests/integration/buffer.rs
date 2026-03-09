// Buffer type tests
//
// Display formatting and byte-level Unicode tests that require Rust APIs.
// Basic operation tests migrated to tests/elle/buffer.lisp.

use crate::common::eval_source;

#[test]
fn test_buffer_display() {
    let result = eval_source(r#"@"hello""#).unwrap();
    let display = format!("{}", result);
    assert_eq!(display, r#"@"hello""#);
}

#[test]
fn test_buffer_display_empty() {
    let result = eval_source(r#"@"""#).unwrap();
    let display = format!("{}", result);
    assert_eq!(display, r#"@"""#);
}

#[test]
fn test_buffer_get_unicode() {
    // Buffer with UTF-8 multi-byte character
    let result = eval_source(r#"(get @"café" 3)"#).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "é");
}

#[test]
fn test_buffer_get_unicode_index() {
    // Character indexing, not byte indexing
    let result = eval_source(r#"(get @"café" 0)"#).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "c");
}
