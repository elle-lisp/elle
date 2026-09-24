// audited: 2026-09-23
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! What a branch and a block leave behind: a write inside a narrowed branch
//! outlives the branch, and a block's type joins the values its `break`s carry.

use super::{compile_booted, run_booted};
use crate::value::Value;

/// The loop test narrows `cur` to a pair for the body, and the body writes
/// `cur`. The read after the loop sees that write: the empty list.
///
/// The counter-factual: the narrowing's end put back the pair `cur` held
/// before the loop and dropped the write, so the fold decided `(pair? cur)`
/// after the loop and answered `:stopped`. Without the `(< n 10)` bound the
/// same fold decided the loop test itself, and the loop never ended.
#[test]
fn a_write_in_a_guarded_branch_outlives_the_branch() {
    let src = "(defn walk [& xs]
                 (def @cur xs)
                 (def @n 0)
                 (while (and (pair? cur) (< n 10))
                   (assign n (+ n 1))
                   (assign cur (rest cur)))
                 (if (pair? cur) :stopped :walked))
               (walk 1 2 3)";
    assert_eq!(run_booted(src), Value::keyword("walked"), "{src}");
}

/// A `(type-of v)` arm overrides `v`'s type for its body, and this arm writes
/// `v`. The loop keeps `v` in a cell, so every read after the loop is the one
/// binding, and it may hold the string.
///
/// The counter-factual: the arm's end put back the int and dropped the write,
/// so `(int? v)` decided true and the program answered `:int`.
#[test]
fn a_write_in_a_type_of_arm_outlives_the_arm() {
    let src = "(defn f [x]
                 (def @v x)
                 (def @n 0)
                 (while (< n 2)
                   (match (type-of v) :integer (assign v \"s\") _ nil)
                   (assign n (+ n 1)))
                 (if (int? v) :int :other))
               (f 1)";
    assert_eq!(run_booted(src), Value::keyword("other"), "{src}");
}

/// The block hands back the `break`'s string whenever `x` is positive, so `v`
/// is an int or a string, and `%bit-and` cannot prove it. The counter-factual:
/// the block was typed by its tail alone, an int, and the site compiled over a
/// value that is a string at run time.
#[test]
fn a_block_joins_the_values_of_its_breaks() {
    let src = "(defn f [x]
                 (let [v (block :b (when (> x 0) (break :b \"s\")) 1)]
                   (%bit-and v 1)))
               (f 5)";
    let err = compile_booted(src).expect_err("v may be the break's string");
    assert!(err.contains("%bit-and"), "{src}: {err}");
    let pick = "(defn pick [x]
                  (let [v (block :b (when (> x 0) (break :b \"s\")) 1)]
                    (if (int? v) :int :other)))
                (pick 5)";
    assert_eq!(run_booted(pick), Value::keyword("other"), "{pick}");
}

/// A `break` leaves no value where it stands, so the `if` whose branch it is
/// takes the other branch's type, and the block joins the two ints. The
/// counter-factual: a `break` typed as unknown made the `if`, and so the block,
/// unknown, and the proven `%bit-and` was rejected.
#[test]
fn a_break_adds_nothing_where_it_stands() {
    let src = "(defn f [x]
                 (let [v (block :b (if (> x 0) (break :b 2) 1))]
                   (%bit-and v 1)))
               (f 5)";
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
}
