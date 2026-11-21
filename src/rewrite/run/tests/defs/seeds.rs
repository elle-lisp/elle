// audited: 2026-11-20
//! Tests for the epoch 17 rule: the binding a def assigns holds the binding of its name before it, or nil.
//!
//! docs/epochs/bodies.md

use super::*;

#[test]
fn a_parameter_seeds_the_binding_and_the_epoch_18_rule_makes_it_a_let() {
    moves(
        "(defn f [c x] (when c (def x 1)) x)",
        "(defn f [c x] (let [@x x] (when c (assign x 1)) x))",
    );
}

#[test]
fn an_earlier_def_in_the_body_or_a_body_around_it_seeds_the_binding() {
    moves(
        "(defn f [c] (def x 0) (when c (def x 1)) x)",
        "(defn f [c] (def x 0) (let [@x x] (when c (assign x 1)) x))",
    );
    moves(
        "(def x 0)\n(defn f [c] (when c (def x 1)) x)",
        "(def x 0)\n(defn f [c] (let [@x x] (when c (assign x 1)) x))",
    );
}

#[test]
fn a_let_or_each_binding_seeds_the_binding() {
    moves(
        "(defn f [c] (let [x 0] (when c (def x 1)) x))",
        "(defn f [c] (let [x 0] (let [@x x] (when c (assign x 1)) x)))",
    );
    moves(
        "(defn f [c xs] (each x xs (when c (def x 1)) (g x)))",
        "(defn f [c xs] (each x xs (let [@x x] (when c (assign x 1)) (g x))))",
    );
}

#[test]
fn a_seeded_binding_in_a_begin_splices_the_begin_and_becomes_a_let() {
    // The counter-factual left `(def @x x)` in the begin, which reads its own
    // name before it binds it.
    moves(
        "(defn f [c x] (begin (g) (when c (def x 1))) x)",
        "(defn f [c x] (g) (let [@x x] (when c (assign x 1)) x))",
    );
}

#[test]
fn a_later_def_in_a_body_around_it_seeds_nothing() {
    moves(
        "(defn f [c] (when c (def x 1)) x)\n(def x 0)",
        "(defn f [c] (def @x nil) (when c (assign x 1)) x)\n(def x 0)",
    );
}

#[test]
fn an_earlier_assign_of_the_name_seeds_the_next_binding() {
    moves(
        "(defn f [c d] (when c (def x 1)) (when d (def x 2)) x)",
        "(defn f [c d] (def @x nil) (when c (assign x 1)) (let [@x x] (when d (assign x 2)) x))",
    );
}

#[test]
fn two_defs_of_a_name_in_one_form_share_one_binding() {
    moves(
        "(defn f [c d] (when c (def x 1) (when d (def x 2))) x)",
        "(defn f [c d] (def @x nil) (when c (assign x 1) (when d (assign x 2))) x)",
    );
}
