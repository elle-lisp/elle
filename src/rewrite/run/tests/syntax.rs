// audited: 2026-11-24
//! Tests for the epoch 19 rewrite of real source text: every syntax operation takes its name under syntax/.
//!
//! docs/macros.md
//! docs/epochs.md

use super::*;

#[test]
fn a_file_at_epoch_18_takes_the_syntax_names() {
    assert_eq!(
        rewritten_from(18, "(defmacro m (x) (syntax-e (syntax-first x)))\n"),
        "(defmacro m (x) (syntax/e (syntax/first x)))\n"
    );
}

#[test]
fn a_meta_spelling_takes_the_syntax_name() {
    assert_eq!(
        rewritten_from(18, "(def build meta/datum->syntax)\n"),
        "(def build syntax/<-datum)\n"
    );
}

#[test]
fn a_comment_that_names_an_old_operation_is_the_authors_text() {
    assert_eq!(
        rewritten_from(18, "# syntax-e unwraps\n(syntax-e x)\n"),
        "# syntax-e unwraps\n(syntax/e x)\n"
    );
}
