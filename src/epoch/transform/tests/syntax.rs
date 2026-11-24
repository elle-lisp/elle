// audited: 2026-11-24
//! Tests for the epoch 19 migration of the tree: every syntax operation takes its name under syntax/.
//!
//! docs/macros.md
//! docs/epochs.md

use super::migrated_text;

/// `source` migrated from epoch 18, the last epoch with the bare names.
fn from_18(source: &str) -> String {
    migrated_text(source, 18, 19).unwrap()
}

#[test]
fn each_operation_takes_its_syntax_name_from_the_bare_and_the_meta_spelling() {
    let cases = [
        ("syntax->datum", "syntax/->datum"),
        ("datum->syntax", "syntax/<-datum"),
        ("syntax-e", "syntax/e"),
        ("syntax-pair?", "syntax/pair?"),
        ("syntax-list?", "syntax/list?"),
        ("syntax-symbol?", "syntax/symbol?"),
        ("syntax-keyword?", "syntax/keyword?"),
        ("syntax-nil?", "syntax/nil?"),
        ("syntax->list", "syntax/->list"),
        ("syntax-first", "syntax/first"),
        ("syntax-rest", "syntax/rest"),
    ];
    for (old, new) in cases {
        assert_eq!(from_18(&format!("({old} x)")), format!("({new} x)"));
        assert_eq!(from_18(&format!("(meta/{old} x)")), format!("({new} x)"));
    }
}

#[test]
fn a_name_passed_as_a_value_is_renamed() {
    assert_eq!(from_18("(map syntax->datum xs)"), "(map syntax/->datum xs)");
}

#[test]
fn a_quasiquote_template_calls_the_new_name() {
    // A template is code the macro returns, so the expansion would otherwise
    // call a name the current epoch no longer binds.
    let text = from_18("(defmacro m (x) `(f ,(syntax-e x)))");
    assert!(text.contains("(syntax/e x)"), "{text}");
    assert!(!text.contains("syntax-e"), "{text}");
}

#[test]
fn a_quoted_name_is_data_and_keeps_its_spelling() {
    let text = from_18("(def names '(syntax-e datum->syntax))");
    assert!(text.contains("syntax-e"), "{text}");
    assert!(!text.contains("syntax/"), "{text}");
}

#[test]
fn the_meta_operations_that_read_no_syntax_keep_their_names() {
    let source = "(meta/gensym)\n(meta/location)\n(meta/origin f)";
    assert_eq!(from_18(source), source);
}

#[test]
fn no_epoch_before_19_renames_an_operation() {
    let source = "(syntax-e (meta/syntax-first x))";
    assert_eq!(migrated_text(source, 0, 18).unwrap(), source);
}

#[test]
fn a_file_at_epoch_17_reaches_the_syntax_names() {
    assert_eq!(
        migrated_text("(syntax-first x)", 17, 19).unwrap(),
        "(syntax/first x)"
    );
}
