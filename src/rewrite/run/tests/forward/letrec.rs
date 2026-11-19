// audited: 2026-11-18
//! Tests for the epoch 18 rule: the forward reads no move carries become one letrec, in text and tree alike.
//!
//! docs/epochs.md

use super::*;

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
fn functions_that_call_each_other_move_together_then_bind_in_a_letrec() {
    carries(
        "(defn main [] (ev? 4)) \
         (defn ev? [n] (if (= n 0) true (od? (- n 1)))) \
         (defn od? [n] (if (= n 0) false (ev? (- n 1)))) (main)",
        "(letrec [ev? (fn [n] (if (= n 0) true (od? (- n 1)))) \
         od? (fn [n] (if (= n 0) false (ev? (- n 1))))] \
         (defn main [] (ev? 4)) (main))",
    );
}

#[test]
fn a_def_between_the_read_and_its_def_joins_the_letrec() {
    // `k` is no function, so it cannot move, and `g`, which reads it, stays
    // after it.
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
fn a_mutable_def_does_not_move() {
    // An `assign` the def passed over would run before the def resets it.
    carries(
        "(defn f [] (g)) (var g (fn [] 1)) (f)",
        "(letrec [f (fn [] (g)) @g (fn [] 1)] (f))",
    );
    carries(
        "(defn f [] (g)) (def @g (fn [] 1)) (f)",
        "(letrec [f (fn [] (g)) @g (fn [] 1)] (f))",
    );
}

#[test]
fn a_name_the_body_defines_twice_does_not_move() {
    carries(
        "(defn f [] (g)) (defn g [] 1) (defn g [] 2) (f)",
        "(letrec [f (fn [] (g)) g (fn [] 1)] (defn g [] 2) (f))",
    );
}

#[test]
fn the_last_form_does_not_move_and_the_letrec_reads_its_name() {
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
fn a_later_run_stands_in_the_body_of_an_earlier_one() {
    carries(
        "(defn a [] (b)) (defn b [] (a)) (defn c [] (d)) (defn d [] (c)) (c)",
        "(letrec [a (fn [] (b)) b (fn [] (a))] (letrec [c (fn [] (d)) d (fn [] (c))] (c)))",
    );
}

#[test]
fn functions_that_call_each_other_move_up_to_the_first_of_them() {
    carries(
        "(defn a [] (c)) (defn b [] (d)) (defn c [] (a)) (defn d [] (b)) (a)",
        "(letrec [a (fn [] (c)) c (fn [] (a))] (letrec [b (fn [] (d)) d (fn [] (b))] (a)))",
    );
}

#[test]
fn runs_that_overlap_join() {
    carries(
        "(var a (fn [] c)) (var b (fn [] d)) (var c (fn [] a)) (var d (fn [] b)) (a)",
        "(letrec [@a (fn [] c) @b (fn [] d) @c (fn [] a) @d (fn [] b)] (a))",
    );
}

#[test]
fn a_def_the_epoch_17_rule_moves_reaches_the_letrec() {
    // `g` is the body's last form, so it stays.
    carried_from(
        16,
        "(defn f [c] (when c (def x 1) (g x))) (defn g [y] y)",
        "(letrec [f (fn [c] (when c (let [x 1] (g x)))) g (fn [y] y)] g)",
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
