// audited: 2026-11-20
//! Tests for the epoch 17 rule: a def that may not run, and that a read after its form reaches, becomes an assign.
//!
//! docs/epochs/bodies.md
//!
//! The counter-factual left each such def as written, so the compiler refused
//! it at epoch 17.

use super::*;

#[test]
fn a_def_read_after_its_form_assigns_a_binding_made_before_the_form() {
    moves(
        "(defn f [c] (when c (def x 1)) x)",
        "(defn f [c] (def @x nil) (when c (assign x 1)) x)",
    );
    moves(
        "(defn f [c] (when c (def x 1) (g x)) x)",
        "(defn f [c] (def @x nil) (when c (assign x 1) (g x)) x)",
    );
}

#[test]
fn a_var_a_mutable_def_and_a_defn_assign_too() {
    moves(
        "(defn f [c] (when c (var x 1)) x)",
        "(defn f [c] (def @x nil) (when c (assign x 1)) x)",
    );
    moves(
        "(defn f [c] (when c (def @x 1)) x)",
        "(defn f [c] (def @x nil) (when c (assign x 1)) x)",
    );
    moves(
        "(defn f [c] (when c (defn g [] 1)) (g))",
        "(defn f [c] (def @g nil) (when c (assign g (fn [] 1))) (g))",
    );
}

#[test]
fn a_def_in_an_operand_assigns_from_the_form_that_holds_it() {
    moves(
        "(defn f [c] (def k (if c (def x 1) 2)) x)",
        "(defn f [c] (def @x nil) (def k (if c (assign x 1) 2)) x)",
    );
}

#[test]
fn a_def_at_the_file_level_assigns_a_binding_on_a_line_of_its_own() {
    moves(
        "(when c (def x 1))\n(g x)",
        "(def @x nil)\n(when c (assign x 1))\n(g x)",
    );
}

#[test]
fn a_form_in_a_begin_takes_the_binding_in_the_begin() {
    moves(
        "(defn f [c] (begin (g) (when c (def x 1))) x)",
        "(defn f [c] (begin (g) (def @x nil) (when c (assign x 1))) x)",
    );
}

#[test]
fn a_def_in_a_form_that_passes_it_to_the_body_assigns() {
    moves(
        "(defn f [c] (if c (def x 1) 2) x)",
        "(defn f [c] (def @x nil) (if c (assign x 1) 2) x)",
    );
    moves(
        "(defn f [c] (cond (g) 2 c (def x 1)) x)",
        "(defn f [c] (def @x nil) (cond (g) 2 c (assign x 1)) x)",
    );
    moves(
        "(defn f [c] (and c (def x 1)) x)",
        "(defn f [c] (def @x nil) (and c (assign x 1)) x)",
    );
    moves(
        "(defn f [c] (while c (def x 1)) x)",
        "(defn f [c] (def @x nil) (while c (assign x 1)) x)",
    );
    moves(
        "(defn f [c] (forever (when c (def x 1)) (break)) x)",
        "(defn f [c] (def @x nil) (forever (when c (assign x 1)) (break)) x)",
    );
    moves(
        "(defn f [c] (assign k (if c (def x 1) 2)) x)",
        "(defn f [c] (def @x nil) (assign k (if c (assign x 1) 2)) x)",
    );
    moves(
        "(defn f [c] (match (when c (def x 1)) _ 3) x)",
        "(defn f [c] (def @x nil) (match (when c (assign x 1)) _ 3) x)",
    );
    moves(
        "(defn f [c] (parameterize ((p (when c (def x 1)))) 3) x)",
        "(defn f [c] (def @x nil) (parameterize ((p (when c (assign x 1)))) 3) x)",
    );
}

#[test]
fn a_def_in_a_form_that_keeps_it_inside_stays() {
    // Up to epoch 16 the read after the form found the binding before the
    // def, so an assign would hand it the def's value. The counter-factual
    // assigned each of these.
    stays("(defn f [c] (let [k (when c (def x 1))] k) x)");
    stays("(defn f [c] (each i (when c (def x 1) [1]) (g)) x)");
    stays("(defn f [c] (repeat 1 (when c (def x 1))) x)");
    stays("(defn f [c] (match 1 y when (when c (def x 1)) 3 _ 4) x)");
    stays("(defn f [c] (if-let [y (h)] (when c (def x 1)) nil) x)");
    stays("(defn f [c] (protect (when c (def x 1))) x)");
}

// --- the binding before it ---

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

// --- the rule among the others ---

#[test]
fn a_def_the_assign_leaves_behind_moves_into_a_let() {
    // Before the assign, `y` held `x` back: the let would hide `y` from the
    // read after the form.
    moves(
        "(defn f [c] (when c (def x 1) (g) (def y 2)) y)",
        "(defn f [c] (def @y nil) (when c (let [x 1] (g) (assign y 2))) y)",
    );
}

#[test]
fn a_def_that_reads_the_assigned_name_becomes_a_let() {
    moves(
        "(defn f [c] (when c (def x 1)) (def x (+ x 1)) x)",
        "(defn f [c] (def @x nil) (when c (assign x 1)) (let [x (+ x 1)] x))",
    );
}

#[test]
fn a_direct_read_before_a_later_def_of_the_name_is_a_read() {
    // The read in `h` comes before `h`'s own def, so it reads past that def
    // to the def in the `when`.
    moves(
        "(defn f [c] (when c (def x 1)) (defn h [] (g x) (def x 2) x) (h))",
        "(defn f [c] (def @x nil) (when c (assign x 1)) (defn h [] (g x) (def x 2) x) (h))",
    );
}

// --- a def that stays ---

#[test]
fn a_def_that_binds_a_pattern_stays() {
    stays("(defn f [c v] (when c (def [a b] v)) a)");
    stays("(defn f [c v] (when c (def [a b] v) (g a)) b)");
}

#[test]
fn a_read_in_the_form_outside_the_def_s_sequence_holds_it_back() {
    stays("(defn f [c] (when (g x) (def x 1)) x)");
    stays("(defn f [c] (when c (g x) (def x 1)) x)");
}

#[test]
fn a_def_in_a_template_stays() {
    // The binding would land beside the template's one form.
    stays("(defmacro m [c] `(begin (when ,c (def x 1)) x))");
}

// --- the text alone ---

#[test]
fn the_binding_takes_a_line_of_its_own_at_the_form_s_indent() {
    assert_eq!(
        rewritten_from(16, "(defn f [c]\n  (when c\n    (def x 1))\n  x)\n"),
        "(defn f [c]\n  (def @x nil)\n  (when c\n    (assign x 1))\n  x)\n"
    );
}
