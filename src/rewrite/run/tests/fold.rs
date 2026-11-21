// audited: 2026-11-20
//! Tests for the epoch 16 fold: a module of several forms folds into its last fn form, by `elle rewrite` and by a loader alike.
//!
//! docs/epochs.md
//!
//! Each shared case runs the rule twice, over source text and over the tree a
//! loader reads, and holds both to one answer. The text keeps the author's
//! layout, so the shared cases write a module on one line.

use super::*;

/// `body`, written at epoch 15, read as a module and migrated by the tree
/// rules, printed back one form to a line.
fn module_migrated_from(from: u64, body: &str) -> String {
    let arena = crate::syntax::thread_arena();
    let mut forms = crate::reader::read_syntax_all(arena, body, "<test>").unwrap();
    crate::epoch::transform::migrate_module(&arena, &mut forms, from, CURRENT_EPOCH).unwrap();
    forms
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `body`, a module written at epoch 15, becomes `expected` through the text
/// rewrite and through a loader's tree migration.
fn folds(body: &str, expected: &str) {
    assert_eq!(
        rewritten_from(15, &format!("{body}\n")),
        format!("{expected}\n"),
        "the text rewrite"
    );
    assert_eq!(
        module_migrated_from(15, body),
        expected,
        "the tree migration"
    );
}

#[test]
fn the_forms_before_the_function_move_into_its_body() {
    folds(
        "(def greeting \"Hi\") (defn hi [n] (string greeting n)) (fn [] {:hi hi})",
        "(fn [] (def greeting \"Hi\") (defn hi [n] (string greeting n)) {:hi hi})",
    );
}

#[test]
fn a_docstring_stays_first() {
    folds(
        "(def a 1) (fn [] \"The module.\" {:a a})",
        "(fn [] \"The module.\" (def a 1) {:a a})",
    );
}

#[test]
fn a_form_that_spells_a_parameter_wraps_instead() {
    folds(
        "(def x 10) (fn [x] {:x x})",
        "(fn [& args] (def x 10) (apply (fn [x] {:x x}) args))",
    );
}

#[test]
fn a_parenthesized_parameter_list_is_kept_as_written() {
    folds(
        "(def a 1) (fn (&named sep) {:a a :sep sep})",
        "(fn (&named sep) (def a 1) {:a a :sep sep})",
    );
}

#[test]
fn the_layout_and_comments_between_the_forms_stay() {
    // The text rewrite edits the function's head and nothing else, so `elle
    // fmt` re-indents the body afterwards.
    assert_eq!(
        rewritten_from(
            15,
            "# header\n(def a 1)\n# the exports\n(fn []\n  {:a a})\n"
        ),
        "# header\n(fn [] (def a 1)\n# the exports\n{:a a})\n"
    );
}

#[test]
fn a_program_is_not_folded() {
    // A program's last form is not a function: the forms stay, and the tag
    // moves.
    assert_eq!(
        rewritten_from(15, "(def a 1)\n(println a)\n"),
        "(def a 1)\n(println a)\n"
    );
}

#[test]
fn a_file_at_epoch_16_is_not_folded() {
    // Epoch 16 made a module one form, so its files are not old modules.
    let source = "(elle/epoch 16)\n(def a 1)\n(fn [] {:a a})\n";
    let (new_source, _) = rewrite_file(source, "<test>").unwrap().unwrap();
    assert_eq!(
        new_source,
        format!("(elle/epoch {CURRENT_EPOCH})\n(def a 1)\n(fn [] {{:a a}})\n")
    );
}
