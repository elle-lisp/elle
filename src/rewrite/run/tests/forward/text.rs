// audited: 2026-11-18
//! Tests for the epoch 18 rule over source text alone: what a moved def takes with it, and the layout and comments it keeps.
//!
//! docs/epochs.md

use super::*;

#[test]
fn a_moved_def_takes_the_comment_lines_directly_above_it() {
    assert_eq!(
        rewritten_from(
            17,
            "(defn f [] (g))\n\n# the answer\n(defn g [] 1)\n\n(f)\n"
        ),
        "# the answer\n(defn g [] 1)\n\n(defn f [] (g))\n\n(f)\n"
    );
}

#[test]
fn a_comment_a_blank_line_parts_from_the_def_stays() {
    assert_eq!(
        rewritten_from(17, "(defn f [] (g))\n# about f\n\n(defn g [] 1)\n(f)\n"),
        "(defn g [] 1)\n(defn f [] (g))\n# about f\n\n(f)\n"
    );
}

#[test]
fn a_moved_def_takes_the_comment_that_ends_its_line() {
    assert_eq!(
        rewritten_from(17, "(defn f [] (g))\n(defn g [] 1) # one\n(f)\n"),
        "(defn g [] 1) # one\n(defn f [] (g))\n(f)\n"
    );
}

#[test]
fn a_moved_def_lands_above_the_comment_lines_of_the_form_it_moves_above() {
    assert_eq!(
        rewritten_from(17, "# calls g\n(defn f [] (g))\n(defn g [] 1)\n(f)\n"),
        "(defn g [] 1)\n# calls g\n(defn f [] (g))\n(f)\n"
    );
}

#[test]
fn a_moved_def_keeps_its_lines_in_a_function_body() {
    assert_eq!(
        rewritten_from(
            17,
            "(defn outer []\n  (defn f []\n    (g))\n  (defn g []\n    1)\n  (f))\n"
        ),
        "(defn outer []\n  (defn g []\n    1)\n  (defn f []\n    (g))\n  (f))\n"
    );
}

#[test]
fn a_def_in_a_function_moves_inside_a_def_that_moves() {
    // The edits inside `g` travel with it.
    assert_eq!(
        rewritten_from(
            17,
            "(defn f [] (g))\n(defn g []\n  (defn a [] (b))\n  (defn b [] 1)\n  (a))\n(f)\n"
        ),
        "(defn g []\n  (defn b [] 1)\n  (defn a [] (b))\n  (a))\n(defn f [] (g))\n(f)\n"
    );
}

#[test]
fn the_letrec_keeps_the_layout_and_the_comments() {
    assert_eq!(
        rewritten_from(17, "(defn f []\n  (g)) # calls g\n(defn g [] (f))\n(f)\n"),
        "(letrec [f (fn []\n  (g)) # calls g\ng (fn [] (f))]\n(f))\n"
    );
}
