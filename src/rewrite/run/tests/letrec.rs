// audited: 2026-11-18
//! Tests for the epoch 18 rule: the defs from a forward read through the def it reads become one letrec, in text and tree alike.
//!
//! docs/epochs.md
//! docs/bindings.md
//!
//! Each case runs the rule twice, over source text and over the syntax tree,
//! and holds both to one answer, as the epoch 17 rule's cases do.

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

// --- a forward read ---

#[test]
fn a_function_that_reads_a_later_def_binds_with_it_in_a_letrec() {
    carries(
        "(defn f [] (g)) (defn g [] 1) (f)",
        "(letrec [f (fn [] (g)) g (fn [] 1)] (f))",
    );
}

#[test]
fn a_function_body_and_a_let_body_carry_as_the_file_does() {
    carries(
        "(defn outer [] (defn f [] (g)) (defn g [] 1) (f))",
        "(defn outer [] (letrec [f (fn [] (g)) g (fn [] 1)] (f)))",
    );
    carries(
        "(fn [] (defn f [] (g)) (defn g [] 1) (f))",
        "(fn [] (letrec [f (fn [] (g)) g (fn [] 1)] (f)))",
    );
    carries(
        "(defn outer [] (let [b 1] (defn f [] (g)) (defn g [] b) (f)))",
        "(defn outer [] (let [b 1] (letrec [f (fn [] (g)) g (fn [] b)] (f))))",
    );
}

#[test]
fn the_defs_before_the_reading_def_stay_where_they_are() {
    carries(
        "(def a 1) (defn f [] (g)) (defn g [] a) (f)",
        "(def a 1) (letrec [f (fn [] (g)) g (fn [] a)] (f))",
    );
}

#[test]
fn a_def_between_the_read_and_its_def_joins_the_letrec() {
    carries(
        "(defn f [] (g)) (def k 2) (defn g [] k) (f)",
        "(letrec [f (fn [] (g)) k 2 g (fn [] k)] (f))",
    );
}

#[test]
fn a_var_binds_its_name_with_an_at() {
    carries(
        "(defn f [] (g)) (var n 0) (defn g [] n) (f)",
        "(letrec [f (fn [] (g)) @n 0 g (fn [] n)] (f))",
    );
    carries(
        "(defn f [] (g)) (def @n 0) (defn g [] n) (f)",
        "(letrec [f (fn [] (g)) @n 0 g (fn [] n)] (f))",
    );
}

#[test]
fn a_run_that_ends_its_body_reads_its_last_name() {
    // The body's value was the last def's. A letrec with no body answers nil.
    carries(
        "(defn outer [] (defn f [] (g)) (defn g [] 1))",
        "(defn outer [] (letrec [f (fn [] (g)) g (fn [] 1)] g))",
    );
    carries(
        "(def f (fn [] (g))) (def g (fn [] 1))",
        "(letrec [f (fn [] (g)) g (fn [] 1)] g)",
    );
}

#[test]
fn a_read_in_a_lambda_nested_in_the_value_is_a_forward_read() {
    carries(
        "(defn f [xs] (map (fn [x] (g x)) xs)) (defn g [x] x) (f [1])",
        "(letrec [f (fn [xs] (map (fn [x] (g x)) xs)) g (fn [x] x)] (f [1]))",
    );
}

#[test]
fn functions_that_call_each_other_bind_in_one_letrec() {
    carries(
        "(defn ev? [n] (if (= n 0) true (od? (- n 1)))) \
         (defn od? [n] (if (= n 0) false (ev? (- n 1)))) (ev? 4)",
        "(letrec [ev? (fn [n] (if (= n 0) true (od? (- n 1)))) \
         od? (fn [n] (if (= n 0) false (ev? (- n 1))))] (ev? 4))",
    );
}

#[test]
fn runs_that_overlap_join() {
    carries(
        "(defn a [] (c)) (defn b [] (d)) (defn c [] 1) (defn d [] 2) (a)",
        "(letrec [a (fn [] (c)) b (fn [] (d)) c (fn [] 1) d (fn [] 2)] (a))",
    );
}

#[test]
fn a_later_run_stands_in_the_body_of_an_earlier_one() {
    carries(
        "(defn a [] (b)) (defn b [] 1) (defn c [] (d)) (defn d [] 2) (c)",
        "(letrec [a (fn [] (b)) b (fn [] 1)] (letrec [c (fn [] (d)) d (fn [] 2)] (c)))",
    );
}

// --- a def that reads its own name ---

#[test]
fn a_def_whose_value_reads_its_own_name_becomes_a_let() {
    // Up to epoch 17 the read found the binding before the def. A let's value
    // finds it still; a def's value no longer does.
    carries(
        "(defn f [] (def x 1) (def x (+ x 1)) x)",
        "(defn f [] (def x 1) (let [x (+ x 1)] x))",
    );
    carries("(def n 1) (def n (+ n 1))", "(def n 1) (let [n (+ n 1)] n)");
    carries(
        "(def @n 1) (def @n (+ n 1)) (assign n 3) n",
        "(def @n 1) (let [@n (+ n 1)] (assign n 3) n)",
    );
}

// --- the rule among the others ---

#[test]
fn a_def_the_epoch_17_rule_moves_reaches_the_letrec() {
    carried_from(
        16,
        "(defn f [c] (when c (def x 1) (g x))) (defn g [y] y)",
        "(letrec [f (fn [c] (when c (let [x 1] (g x)))) g (fn [y] y)] g)",
    );
}

// --- the text alone ---

#[test]
fn the_text_rewrite_keeps_the_layout_and_the_comments() {
    assert_eq!(
        rewritten_from(17, "(defn f []\n  (g)) # calls g\n(defn g [] 1)\n(f)\n"),
        "(letrec [f (fn []\n  (g)) # calls g\ng (fn [] 1)]\n(f))\n"
    );
}

mod stays;
