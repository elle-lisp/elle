// audited: 2026-11-20
//! Tests for the epoch 16 rewrite of source text: `import-file` becomes `import`, and an `include` of a spec is refused.
//!
//! docs/epochs.md
//! docs/include.md

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

#[test]
fn an_include_of_a_spec_is_refused_with_what_to_write() {
    let source = "(elle/epoch 15)\n(include \"std/base64\")\n";
    let err = rewrite_file(source, "<test>").unwrap_err();
    assert!(err.contains("`include` has been removed"), "{err}");
    assert!(err.contains("(include-file \"path\")"), "{err}");
}
