// @string type tests
//
// Display formatting and byte-level Unicode tests that require Rust APIs.
// Basic operation tests migrated to tests/elle/string.lisp.

use crate::common::eval_source;

#[test]
fn test_buffer_display() {
    // The @string is heap-backed; `Display` derefs its pages, so render inside
    // the scope where the runtime (heap) is still alive.
    eval_source(r#"@"hello""#, |r| {
        assert_eq!(format!("{}", r.unwrap()), r#"@"hello""#);
    });
}

#[test]
fn test_buffer_display_empty() {
    eval_source(r#"@"""#, |r| {
        assert_eq!(format!("{}", r.unwrap()), r#"@"""#);
    });
}

#[test]
fn test_buffer_get_unicode() {
    // @string with UTF-8 multi-byte character
    eval_source(r#"(get @"café" 3)"#, |r| {
        assert_eq!(r.unwrap().with_string(|s| s.to_string()).unwrap(), "é");
    });
}

#[test]
fn test_buffer_get_unicode_index() {
    // Character indexing, not byte indexing
    eval_source(r#"(get @"café" 0)"#, |r| {
        assert_eq!(r.unwrap().with_string(|s| s.to_string()).unwrap(), "c");
    });
}
