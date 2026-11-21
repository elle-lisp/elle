// audited: 2026-11-20
//! Tests for the epoch 17 rule: a def outside a body moves into a let, by `elle rewrite` and by the compiler alike.
//!
//! docs/epochs/bodies.md
//!
//! Each case runs the rule twice, over source text and over the syntax tree,
//! and holds both to one answer. The two consumers share one reading of where
//! a def stands, so a case that passes one and fails the other has found a
//! placer that disagrees with the reading.

use super::*;

/// `body`, written at epoch 16, becomes `expected` through the text rewrite
/// and through the tree migration.
fn moves(body: &str, expected: &str) {
    assert_eq!(
        rewritten_from(16, &format!("{body}\n")),
        format!("{expected}\n"),
        "the text rewrite"
    );
    assert_eq!(
        migrated_from(16, body).unwrap(),
        expected,
        "the tree migration"
    );
}

/// `body`, written at epoch 16, stays as written through both.
fn stays(body: &str) {
    moves(body, body);
}

// --- a form that runs its operands in sequence ---

#[test]
fn a_def_in_a_when_moves_into_a_let_to_the_end_of_the_when() {
    moves(
        "(defn f [c] (when c (def x 1) (g x)))",
        "(defn f [c] (when c (let [x 1] (g x))))",
    );
}

#[test]
fn the_forms_before_the_def_stay_outside_the_let() {
    moves(
        "(defn f [c] (unless c (g) (def x 1) (h x)))",
        "(defn f [c] (unless c (g) (let [x 1] (h x))))",
    );
}

#[test]
fn a_run_of_defs_becomes_one_let() {
    moves(
        "(defn f [] (while (t) (def a 1) (var b 2) (def @c 3) (g a b c)))",
        "(defn f [] (while (t) (let [a 1 @b 2 @c 3] (g a b c))))",
    );
}

#[test]
fn a_defn_binds_its_name_to_a_fn() {
    moves(
        "(defn f [c] (when c (defn g [y] (+ y 1)) (g 2)))",
        "(defn f [c] (when c (let [g (fn [y] (+ y 1))] (g 2))))",
    );
}

#[test]
fn a_def_after_the_run_already_stands_in_the_let_body() {
    moves(
        "(defn f [c] (when c (def x 1) (g x) (def y 2) (h y)))",
        "(defn f [c] (when c (let [x 1] (g x) (def y 2) (h y))))",
    );
}

#[test]
fn a_run_that_ends_its_form_reads_its_last_name() {
    // The form's value was the def's. A let with no body would answer nil.
    moves(
        "(defn f [c] (when c (def x 1)))",
        "(defn f [c] (when c (let [x 1] x)))",
    );
    moves(
        "(defn f [c] (when c (var x 1)))",
        "(defn f [c] (when c (let [@x 1] x)))",
    );
}

#[test]
fn forever_and_repeat_run_their_forms_as_a_while_does() {
    moves(
        "(defn f [] (forever (def x (g)) (h x)))",
        "(defn f [] (forever (let [x (g)] (h x))))",
    );
    moves(
        "(defn f [] (repeat 3 (def x (g)) (h x)))",
        "(defn f [] (repeat 3 (let [x (g)] (h x))))",
    );
}

// --- an operand ---

#[test]
fn a_def_in_a_branch_binds_its_name_and_reads_it() {
    moves(
        "(defn f [c] (if c (def x 1) 2))",
        "(defn f [c] (if c (let [x 1] x) 2))",
    );
    moves(
        "(defn f [c] (if c (var x 1) 2))",
        "(defn f [c] (if c (let [@x 1] x) 2))",
    );
    moves(
        "(defn f [c] (if c (def @x 1) 2))",
        "(defn f [c] (if c (let [@x 1] x) 2))",
    );
    moves(
        "(defn f [c] (if c (defn g [] 1) 2))",
        "(defn f [c] (if c (let [g (fn [] 1)] g) 2))",
    );
}

#[test]
fn a_def_in_a_clause_or_a_short_circuit_operand_moves_alone() {
    moves(
        "(defn f [n] (cond (= n 0) (def z :zero) :other))",
        "(defn f [n] (cond (= n 0) (let [z :zero] z) :other))",
    );
    moves(
        "(defn f [c] (and c (def x 1)))",
        "(defn f [c] (and c (let [x 1] x)))",
    );
}

#[test]
fn a_begin_outside_a_body_becomes_the_let() {
    moves(
        "(defn f [] (g (begin (def x 1) x)))",
        "(defn f [] (g (let [x 1] x)))",
    );
    moves(
        "(def k (do (defn inner [] 1) (inner)))",
        "(def k (let [inner (fn [] 1)] (inner)))",
    );
    moves(
        "(defn f [] (let [a (begin (def x 1) (+ x 1))] a))",
        "(defn f [] (let [a (let [x 1] (+ x 1))] a))",
    );
}

#[test]
fn a_begin_with_a_form_before_the_def_stays_around_the_let() {
    moves(
        "(defn f [] (g (begin (h) (def x 1) x)))",
        "(defn f [] (g (begin (h) (let [x 1] x))))",
    );
}

#[test]
fn a_def_in_the_value_of_a_def_that_moves_moves_too() {
    moves(
        "(defn f [c d] (when c (def x (if d (def y 1) 2)) (g x)))",
        "(defn f [c d] (when c (let [x (if d (let [y 1] y) 2)] (g x))))",
    );
}

// --- quotation ---

#[test]
fn a_template_is_rewritten_as_code() {
    moves(
        "(defmacro m [c] `(when ,c (def x 1) (g x)))",
        "(defmacro m [c] `(when ,c (let [x 1] (g x))))",
    );
}

// --- the rule among the others ---

#[test]
fn a_var_from_before_epoch_8_reaches_the_let() {
    let body = "(defn f [c] (when c (var x 1) (g x)))";
    let expected = "(defn f [c] (when c (let [@x 1] (g x))))";
    assert_eq!(
        rewritten_from(7, &format!("{body}\n")),
        format!("{expected}\n")
    );
    assert_eq!(migrated_from(7, body).unwrap(), expected);
}

// --- the text alone ---

#[test]
fn the_text_rewrite_keeps_the_layout_and_the_comments() {
    assert_eq!(
        rewritten_from(
            16,
            "(defn f [c]\n  (when c\n    (def x 1) # one\n    (g x)))\n"
        ),
        "(defn f [c]\n  (when c\n    (let [x 1] # one\n    (g x))))\n"
    );
}

#[test]
fn a_begin_with_a_comment_before_its_def_stays_around_the_let() {
    // The tree holds no comment, so this is the text rewrite's alone: turning
    // the begin into the let would drop the comment with `(begin`.
    assert_eq!(
        rewritten_from(16, "(defn f [] (g (begin # note\n  (def x 1) x)))\n"),
        "(defn f [] (g (begin # note\n  (let [x 1] x))))\n"
    );
}

mod assigns;
mod hides;
mod seeds;
mod stays;
