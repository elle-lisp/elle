// audited: 2026-11-18
//! Tests for the epoch 18 rule: a def a forward read reaches moves above the read, in text and tree alike.
//!
//! docs/epochs.md
//! docs/bindings.md
//!
//! Each case runs the rule twice, over source text and over the syntax tree,
//! and holds both to one answer, as the epoch 17 rule's cases do. The letrec
//! the moves leave is [`letrec`]'s subject, the layout of the text
//! [`text`]'s, and the forms the rule leaves [`stays`]'s.

use super::*;

/// `body`, written at epoch `from`, becomes `expected` through the text
/// rewrite and through the tree migration.
fn carried_from(from: u64, body: &str, expected: &str) {
    assert_eq!(
        rewritten_from(from, &format!("{body}\n")),
        format!("{expected}\n"),
        "the text rewrite"
    );
    assert_eq!(
        migrated_from(from, body).unwrap(),
        printed(expected),
        "the tree migration"
    );
}

/// `source` as the tree migration prints it: one form to a line.
fn printed(source: &str) -> String {
    migrated_from(CURRENT_EPOCH, source).unwrap()
}

/// `body`, written at epoch 17, becomes `expected` through both.
fn carries(body: &str, expected: &str) {
    carried_from(17, body, expected);
}

#[test]
fn a_def_a_function_reads_moves_above_the_function() {
    carries(
        "(defn f [] (g)) (defn g [] 1) (f)",
        "(defn g [] 1) (defn f [] (g)) (f)",
    );
}

#[test]
fn a_function_body_and_a_let_body_carry_as_the_file_does() {
    carries(
        "(defn outer [] (defn f [] (g)) (defn g [] 1) (f))",
        "(defn outer [] (defn g [] 1) (defn f [] (g)) (f))",
    );
    carries(
        "(fn [] (defn f [] (g)) (defn g [] 1) (f))",
        "(fn [] (defn g [] 1) (defn f [] (g)) (f))",
    );
    carries(
        "(defn outer [] (let [b 1] (defn f [] (g)) (defn g [] b) (f)))",
        "(defn outer [] (let [b 1] (defn g [] b) (defn f [] (g)) (f)))",
    );
}

#[test]
fn a_def_whose_value_is_a_fn_form_moves() {
    carries(
        "(defn f [] (g)) (def g (fn [] 1)) (f)",
        "(def g (fn [] 1)) (defn f [] (g)) (f)",
    );
}

#[test]
fn a_literal_def_a_moved_function_reads_moves_with_it() {
    // A literal runs no code, so it may be made earlier.
    carries(
        "(defn f [] (g)) (def k 2) (defn g [] k) (f)",
        "(def k 2) (defn g [] k) (defn f [] (g)) (f)",
    );
}

#[test]
fn a_def_moves_past_a_macro_it_does_not_use() {
    carries(
        "(defn f [] (g)) (defmacro m [] 1) (defn g [] 1) (f)",
        "(defn g [] 1) (defn f [] (g)) (defmacro m [] 1) (f)",
    );
}

#[test]
fn an_alias_moves_with_the_def_it_names() {
    // Above `g`, the alias would read a binding outside the body.
    carries(
        "(defn f [] (h)) (defn g [] 1) (def h g) (f)",
        "(defn g [] 1) (def h g) (defn f [] (h)) (f)",
    );
}

#[test]
fn the_forms_before_the_first_read_stay_where_they_are() {
    carries(
        "(def a 1) (defn f [] (g)) (defn g [] a) (f)",
        "(def a 1) (defn g [] a) (defn f [] (g)) (f)",
    );
}

#[test]
fn a_def_lands_above_the_first_form_that_reads_it() {
    carries(
        "(defn f [] (g)) (defn h [] (g)) (defn g [] 1) (h)",
        "(defn g [] 1) (defn f [] (g)) (defn h [] (g)) (h)",
    );
}

#[test]
fn a_def_a_moved_function_reads_moves_ahead_of_it() {
    // Moved alone, `g` would read `h` before its def.
    carries(
        "(defn f [] (g)) (defn h [] 2) (defn g [] (h)) (f)",
        "(defn h [] 2) (defn g [] (h)) (defn f [] (g)) (f)",
    );
}

#[test]
fn moved_defs_keep_the_order_they_were_written_in() {
    carries(
        "(defn f [] (h) (g)) (defn g [] 1) (defn h [] 2) (f)",
        "(defn g [] 1) (defn h [] 2) (defn f [] (h) (g)) (f)",
    );
}

#[test]
fn a_read_in_a_lambda_nested_in_the_value_is_a_forward_read() {
    carries(
        "(defn f [xs] (map (fn [x] (g x)) xs)) (defn g [x] x) (f [1])",
        "(defn g [x] x) (defn f [xs] (map (fn [x] (g x)) xs)) (f [1])",
    );
}

#[test]
fn a_def_the_epoch_17_rule_moves_reaches_the_rule() {
    carried_from(
        16,
        "(defn f [c] (when c (def x 1) (g x))) (defn g [y] y) (f true)",
        "(defn g [y] y) (defn f [c] (when c (let [x 1] (g x)))) (f true)",
    );
}

#[test]
fn a_def_that_moves_and_a_def_that_reads_its_own_name_carry_together() {
    carries(
        "(defn f [] (g)) (defn g [] 1) (def n 1) (def n (+ n 1)) n",
        "(defn g [] 1) (defn f [] (g)) (def n 1) (let [n (+ n 1)] n)",
    );
}

mod letrec;
mod stays;
mod text;
