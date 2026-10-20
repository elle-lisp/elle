// audited: 2026-10-20
// `elle/tokens` counts a text's tokens in the `o200k_base` encoding, reading every byte as ordinary text.
// rig/overview.md
//
// The counts here were measured apart from the rig, so a primitive that split
// on whitespace, or counted bytes, fails them. The trap is the special token:
// an encoder that honors `<|endoftext|>` prices that spelling as one token,
// and a source that quotes it would cost its reader less than it does.

mod common;

use common::{rig, stderr, stdout};

/// What `(elle/tokens TEXT)` prints, where TEXT is an Elle string literal.
fn tokens(literal: &str) -> String {
    let out = rig(&["-e", &format!("(print (elle/tokens {literal}))")]);
    assert!(
        out.status.success(),
        "(elle/tokens {literal}) answers:\n{}",
        stderr(&out)
    );
    stdout(&out).trim().to_string()
}

#[test]
fn a_text_costs_its_tokens_in_the_o200k_encoding() {
    assert_eq!(tokens("\"hello world\""), "2");
    assert_eq!(tokens("\"\""), "0");
    assert_eq!(
        tokens("\"let walked = reach.charge(edge, pace);\\n\""),
        "10",
        "a line of Rust"
    );
}

#[test]
fn a_special_token_spelled_in_the_text_counts_as_the_text_it_is() {
    assert_eq!(tokens("\"<|endoftext|>\""), "7");
}

#[test]
fn a_value_that_is_no_string_is_refused() {
    let out = rig(&["-e", "(elle/tokens 12)"]);
    assert!(
        !out.status.success(),
        "a number has no tokens:\n{}",
        stdout(&out)
    );
}
