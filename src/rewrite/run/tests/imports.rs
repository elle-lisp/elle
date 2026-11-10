// audited: 2026-11-09
//! Tests for the epoch 16 rewrite of real source text: a call of `import-file` becomes a call of `import`.
//!
//! docs/epochs.md

use super::*;

#[test]
fn a_call_of_import_file_is_rewritten_in_place() {
    // Only the head changes. The comment after the call is the author's.
    assert_eq!(
        rewritten_from(15, "(def b64 ((import-file \"std/base64\"))) # codec\n"),
        "(def b64 ((import \"std/base64\"))) # codec\n"
    );
}

#[test]
fn import_file_passed_as_a_value_is_left_as_written() {
    assert_eq!(
        rewritten_from(
            15,
            "(def b64 ((import-file \"std/base64\")))\n(map import-file ps)\n"
        ),
        "(def b64 ((import \"std/base64\")))\n(map import-file ps)\n"
    );
}
