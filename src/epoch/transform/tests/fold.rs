// audited: 2026-11-20
//! Tests for the epoch 16 fold of the tree: a module of several forms folds into its last fn form.
//!
//! docs/epochs.md
//!
//! The cases the tree shares with `elle rewrite` live with the rewrite's tests,
//! which hold both to one answer. These are the tree's own: where the fold
//! applies, and what it hands the epoch 17 and 18 rules.

use super::*;

/// `source` read as a module, migrated from epoch `from` to epoch `to`, and
/// printed back one form to a line.
fn module_text(source: &str, from: u64, to: u64) -> String {
    let arena = thread_arena();
    let mut forms = crate::reader::read_syntax_all(arena, source, "<test>").unwrap();
    migrate_module(&arena, &mut forms, from, to).unwrap();
    forms
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

const OLD: &str = "(def greeting \"Hi\")\n(defn hi [n] (string greeting n))\n(fn [] {:hi hi})";

#[test]
fn a_module_of_three_forms_folds_into_its_function() {
    assert_eq!(
        module_text(OLD, 15, 16),
        "(fn [] (def greeting \"Hi\") (defn hi [n] (string greeting n)) {:hi hi})"
    );
}

#[test]
fn a_program_of_the_same_forms_does_not_fold() {
    // A program runs its forms; folding them into a function nobody calls
    // would run nothing.
    assert_eq!(migrated_text(OLD, 15, 16).unwrap(), OLD);
}

#[test]
fn the_rule_belongs_to_epoch_16() {
    // A module of epoch 16 was written under the one-form rule, so several
    // forms there are the author's mistake, and the loader refuses them.
    assert_eq!(module_text(OLD, 16, 17), OLD);
}

#[test]
fn a_module_whose_last_form_is_not_a_function_does_not_fold() {
    let source = "(def a 1)\n{:a a}";
    assert_eq!(module_text(source, 15, 16), source);
}

#[test]
fn a_module_of_one_form_is_unchanged() {
    let source = "(fn [] {:a 1})";
    assert_eq!(module_text(source, 15, 16), source);
}

#[test]
fn a_docstring_stays_first_in_the_body() {
    assert_eq!(
        module_text("(def a 1)\n(fn [] \"The module.\" {:a a})", 15, 16),
        "(fn [] \"The module.\" (def a 1) {:a a})"
    );
}

#[test]
fn a_lone_string_is_the_value_not_a_docstring() {
    assert_eq!(
        module_text("(def a 1)\n(fn [] \"s\")", 15, 16),
        "(fn [] (def a 1) \"s\")"
    );
}

#[test]
fn a_form_that_spells_a_parameter_wraps_instead() {
    // Folded into the body, the def would bind x over the parameter, and a
    // call with 5 would answer 10.
    assert_eq!(
        module_text("(def x 10)\n(fn [x] {:x x})", 15, 16),
        "(fn [& args] (def x 10) (apply (fn [x] {:x x}) args))"
    );
}

#[test]
fn a_mutable_or_named_parameter_counts_by_its_name() {
    assert_eq!(
        module_text("(def prefix \"[\")\n(fn [&named @prefix] prefix)", 15, 16),
        "(fn [& args] (def prefix \"[\") (apply (fn [&named @prefix] prefix) args))"
    );
}

#[test]
fn the_wrapper_takes_a_name_no_form_spells() {
    assert_eq!(
        module_text("(def args 1)\n(fn [args] args)", 15, 16),
        "(fn [& args-1] (def args 1) (apply (fn [args] args) args-1))"
    );
}

#[test]
fn the_fold_counts_one_rewrite() {
    let arena = thread_arena();
    let mut forms = crate::reader::read_syntax_all(arena, OLD, "<test>").unwrap();
    assert_eq!(migrate_module(&arena, &mut forms, 15, 16).unwrap(), 1);
    assert_eq!(forms.len(), 1);
}

#[test]
fn the_folded_function_keeps_the_span_of_the_function() {
    // An error in the function's head points where the author wrote it.
    let arena = thread_arena();
    let mut forms = crate::reader::read_syntax_all(arena, OLD, "<test>").unwrap();
    migrate_module(&arena, &mut forms, 15, 16).unwrap();
    assert_eq!((forms[0].span.line, forms[0].span.col), (3, 1));
}

#[test]
fn the_epoch_18_rule_reads_the_folded_body() {
    // f reads g before g's def. Folded first, the two stand in one body, and
    // the epoch 18 rule moves g above f there.
    assert_eq!(
        module_text("(defn f [] (g))\n(defn g [] 1)\n(fn [] {:f f})", 15, 18),
        "(fn [] (defn g [] 1) (defn f [] (g)) {:f f})"
    );
}
