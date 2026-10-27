// audited: 2026-10-27
//! The format_code tests, and the assertions every one of them shares.
//!
//! docs/fmt.md

use super::*;

mod basics;
mod blank;
mod body;
mod comments;
mod examples;
mod forms;
mod inline;
mod margin;
mod placement;
mod wrap;

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

/// Assert that one pass over `input` gives `expected`, that a second pass
/// over `expected` gives it back unchanged, and that every line of it starts
/// right of the open delimiter of each form it sits inside.
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
    let left = margin::lines_left_of_open(&first);
    assert!(
        left.is_empty(),
        "lines at or left of their form's open delimiter: {left:?}\n{first}"
    );
}
