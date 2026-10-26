// audited: 2026-10-25
//! The format_code tests, and the assertions every one of them shares.
//!
//! docs/fmt.md

use super::*;

mod basics;
mod blank;
mod comments;
mod forms;
mod inline;
mod placement;

// Shared helper used by the comment-idempotency tests.
fn assert_idempotent(input: &str) {
    let config = FormatterConfig::default();
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(
        first, second,
        "not idempotent:\n--- first ---\n{}\n--- second ---\n{}",
        first, second
    );
}

/// Assert that one pass over `input` gives `expected`, and that a second pass
/// over `expected` gives it back unchanged.
fn assert_formats(input: &str, expected: &str) {
    let config = FormatterConfig::default();
    let first = format_code(input, &config).unwrap();
    assert_eq!(
        first, expected,
        "first pass:\n--- input ---\n{input}\n--- got ---\n{first}\n--- want ---\n{expected}"
    );
    let second = format_code(&first, &config).unwrap();
    assert_eq!(
        second, first,
        "second pass moved:\n--- first ---\n{first}\n--- second ---\n{second}"
    );
}
