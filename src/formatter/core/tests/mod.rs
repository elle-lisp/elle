use super::*;

mod basics;
mod comments;
mod forms;

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
