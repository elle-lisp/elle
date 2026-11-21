// audited: 2026-11-20
//! Tests for the epoch 17 rule: the defs it leaves as written, in a body, in a quotation, or where a let would move a read.
//!
//! docs/epochs/bodies.md

use super::*;

// --- a read a let would send elsewhere ---

#[test]
fn a_def_read_before_its_form_stays() {
    // The next pass of the loop reads the def the last pass ran, and a
    // function written earlier reads it when called later. Inside a let, both
    // reads would find another binding.
    stays("(defn f [] (while (t) (g y) (def y 1)))");
    stays("(defn f [c] (defn h [] x) (when c (def x 1)) (h))");
}

#[test]
fn a_def_whose_own_value_or_an_earlier_binding_reads_it_stays() {
    // A let binds a name only after its value, so neither read would reach it.
    stays("(defn f [c] (when c (defn walk [n] (walk n)) (walk 1)))");
    stays("(defn f [c] (when c (def a (fn [] b)) (def b 1) (a)))");
}

#[test]
fn a_later_run_moves_when_an_earlier_one_stays() {
    moves(
        "(defn f [c] (g x) (when c (def x 1) (h) (def y 2) (k y)) x)",
        "(defn f [c] (g x) (when c (def x 1) (h) (let [y 2] (k y))) x)",
    );
}

#[test]
fn a_redefinition_in_the_body_ends_the_search_after_its_value() {
    moves(
        "(defn f [c] (when c (def x 1) (g x)) (def x 2) x)",
        "(defn f [c] (when c (let [x 1] (g x))) (def x 2) x)",
    );
}

#[test]
fn a_function_reading_its_own_def_is_no_read_of_the_name_before() {
    moves(
        "(defn f [c] (when c (def x 1) (g x)) (defn h [] (defn x [n] (x n)) (x 1)))",
        "(defn f [c] (when c (let [x 1] (g x))) (defn h [] (defn x [n] (x n)) (x 1)))",
    );
}

#[test]
fn a_name_another_form_binds_again_is_not_a_read() {
    moves(
        "(defn f [c xs] (when c (def x 1) (g x)) (map (fn [x] x) xs))",
        "(defn f [c xs] (when c (let [x 1] (g x))) (map (fn [x] x) xs))",
    );
    moves(
        "(defn f [c] (when c (def x 1) (g x)) (let [x 2] x))",
        "(defn f [c] (when c (let [x 1] (g x))) (let [x 2] x))",
    );
}

// --- a read another def of the name precedes ---

#[test]
fn two_forms_that_each_define_and_read_a_name_move_apart() {
    // Each read follows its own form's def, so neither reaches the other's.
    // The counter-factual counted each read against the other def, and left
    // both.
    moves(
        "(defn f [] (g (begin (def a 1) a)) (g (begin (def a 1) a)))",
        "(defn f [] (g (let [a 1] a)) (g (let [a 1] a)))",
    );
}

#[test]
fn a_def_in_each_of_two_whens_moves_into_a_let_of_its_own() {
    moves(
        "(defn f [s] (when (p s) (def @subj s) (g subj)) (when (q s) (def @subj s) (h subj)))",
        "(defn f [s] (when (p s) (let [@subj s] (g subj))) (when (q s) (let [@subj s] (h subj))))",
    );
}

#[test]
fn a_def_in_the_body_before_the_form_hides_the_reads_after_it() {
    moves(
        "(defn f [] (def v 1) (g v) (while (t) (def v (h)) (k v)))",
        "(defn f [] (def v 1) (g v) (while (t) (let [v (h)] (k v))))",
    );
}

#[test]
fn a_read_before_the_other_def_still_counts() {
    // The read in the first when comes before that when's def, so it may
    // reach the def in the second.
    stays("(defn f [c] (when c (g a) (def a 1)) (when c (def a 2) a))");
}

#[test]
fn a_read_after_the_begin_that_holds_the_run_still_counts() {
    // The let takes the begin's place and ends with it. Read as one sequence
    // with the when's forms, the begin's own def would hide the read after it.
    stays("(defn f [c] (when c (begin (def x 1)) (g x)))");
}

// --- a def the rule cannot read ---

#[test]
fn a_def_directly_in_a_form_the_rule_does_not_know_stays() {
    stays("(defn f [] (protect (def x 1) x))");
    stays("(defn f [] (my-loop (def x 1) (g x)))");
}

#[test]
fn a_def_whose_name_a_template_fills_in_stays() {
    stays("(defmacro m [n c] `(when ,c (def ,n 1) (g ,n)))");
}

#[test]
fn a_pattern_def_moves_only_with_a_form_after_it() {
    moves(
        "(defn f [c v] (when c (def [a b] v) (g a b)))",
        "(defn f [c v] (when c (let [[a b] v] (g a b))))",
    );
    stays("(defn f [c v] (if c (def [a b] v) 2))");
}

// --- a def already in a body ---

#[test]
fn a_def_in_a_body_stays() {
    stays("(fn [] (def x 1) x)");
    stays("(defn f [] (let [] (def x 1) x))");
    stays("(defn f [xs] (each i xs (def x i) (g x)))");
    stays("(defn f [xs] (each i in xs (def x i) (g x)))");
    stays("(defn f [v] (match v 1 (begin (def x 1) x) _ 0))");
    stays("(defn f [] (block (def x 1) x))");
    stays("(defn f [] (block :out (def x 1) x))");
    stays("(defn f [p] (parameterize ((p 1)) (def x 1) x))");
}

#[test]
fn a_begin_directly_in_a_body_passes_its_defs_to_the_body() {
    stays("(begin (def x 1))\n(g x)");
    stays("(defn f [] (begin (def x 1)) x)");
}

// --- quotation ---

#[test]
fn a_quoted_form_stays() {
    stays("(defn f [] '(when c (def x 1) (g x)))");
}

#[test]
fn a_begin_at_the_top_of_a_template_stays() {
    // The expansion may land in a body, where the defs bind for the forms
    // after the macro's call.
    stays("(defmacro m [] `(begin (def a 1) (def b 2)))");
}
