// audited: 2026-10-05
//! Tests for the epoch 15 migration: each signal concept takes one form, and
//! squelch takes its spec first.
//!
//! docs/epochs.md

use super::super::migrate;
use crate::reader::read_syntax_all;
use crate::syntax::thread_arena;

/// `source` read, migrated from `from` to epoch 15, and printed back.
fn migrated(from: u64, source: &str) -> Result<String, String> {
    let arena = thread_arena();
    let mut forms = read_syntax_all(arena, source, "<test>").unwrap();
    migrate(&arena, &mut forms, from, 15)?;
    Ok(forms
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join("\n"))
}

/// `source` migrated from epoch 14, the last epoch with `silence`.
fn from_14(source: &str) -> String {
    migrated(14, source).unwrap()
}

#[test]
fn silence_with_no_argument_becomes_silent() {
    assert_eq!(from_14("(fn [] (silence) 1)"), "(fn [] (silent!) 1)");
}

#[test]
fn silence_of_a_parameter_becomes_a_silent_bound() {
    // Counter-factual: the walk found the first rule for `silence`, the
    // arity-0 one, and left the arity-1 form alone.
    assert_eq!(
        from_14("(fn [f] (silence f) (f))"),
        "(fn [f] (bound f ||) (f))"
    );
}

#[test]
fn squelch_takes_its_spec_first() {
    assert_eq!(from_14("(squelch f :yield)"), "(squelch :yield f)");
    assert_eq!(
        from_14("(squelch (fn [] (yield 1)) |:yield :io|)"),
        "(squelch |:yield :io| (fn [] (yield 1)))"
    );
}

#[test]
fn a_flipped_squelch_is_flipped_once() {
    // The template keeps its head symbol, so a walk that matched the
    // replacement again would flip it back, and back again, without end.
    assert_eq!(from_14("(squelch f :yield)"), "(squelch :yield f)");
}

#[test]
fn a_squelch_nested_in_an_argument_is_flipped_too() {
    assert_eq!(
        from_14("(squelch (squelch f :yield) :io)"),
        "(squelch :io (squelch :yield f))"
    );
}

#[test]
fn a_rename_from_an_older_epoch_reaches_inside_a_squelch() {
    assert_eq!(
        migrated(9, "(squelch (fn [] (car x)) :yield)").unwrap(),
        "(squelch :yield (fn [] (first x)))"
    );
}

#[test]
fn muffle_has_no_spelling_and_says_what_to_write() {
    let err = migrated(14, "(fn [] (muffle :yield) (yield 1))").unwrap_err();
    assert!(err.contains("has been removed"), "{err}");
    assert!(err.contains("(squelch spec f)"), "{err}");
}

#[test]
fn a_file_at_epoch_15_is_left_alone() {
    assert_eq!(
        migrated(15, "(squelch :yield f)").unwrap(),
        "(squelch :yield f)"
    );
}
