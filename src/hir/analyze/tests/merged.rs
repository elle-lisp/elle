// audited: 2026-11-10
//! A call through a merged binding signals what every binding it may stand for shares.
//!
//! docs/signals/inference.md
//!
//! The counter-factual for each test that names a known signal: a merged
//! binding carried no facts, so a call through it read the unknown callee's
//! signal, every bit a program can raise.

use super::*;

/// Whether `signal` holds every bit an unknown callee may raise.
fn unknown(signal: Signal) -> bool {
    signal.bits.union(Signal::unknown().bits) == signal.bits
}

#[test]
fn both_branches_define_a_silent_function_so_the_call_is_silent() {
    let result = analyze_file("(defn pick [c] (if c (defn f [] 1) (defn f [] 2)) (f))");
    assert_eq!(signal_of(&result, "pick"), Signal::silent());
}

#[test]
fn a_call_through_a_merge_signals_what_either_function_does() {
    let result = analyze_file("(defn pick [c] (if c (defn f [] 1) (defn f [] (yield 2))) (f))");
    assert_eq!(signal_of(&result, "pick"), Signal::yields());
}

#[test]
fn a_skipped_def_merges_with_the_function_before_it() {
    let result = analyze_file("(defn pick [c] (defn f [] 1) (when c (defn f [] 2)) (f))");
    assert_eq!(signal_of(&result, "pick"), Signal::silent());
}

#[test]
fn every_cond_clause_and_its_else_define_so_nil_never_reaches_the_call() {
    let result = analyze_file(
        "(defn pick [n] (cond (%eq n 0) (defn f [] 0) (%eq n 1) (defn f [] 1) (defn f [] 2)) (f))",
    );
    assert_eq!(signal_of(&result, "pick"), Signal::silent());
}

#[test]
fn an_inner_branch_that_defines_on_every_path_covers_its_outer_branch() {
    let result = analyze_file(
        "(defn pick [c d] (if c (if d (defn f [] 1) (defn f [] 2)) (defn f [] 3)) (f))",
    );
    assert_eq!(signal_of(&result, "pick"), Signal::silent());
}

// The tests below guard the other direction: a path that leaves the name
// `nil`, or bindings that disagree, keep the call as wide as the unknown
// callee or the check it needs.

#[test]
fn a_merge_that_may_still_be_nil_is_an_unknown_callee() {
    let result = analyze_file("(defn maybe [c] (when c (defn f [] 1)) (f))");
    assert!(unknown(signal_of(&result, "maybe")));
}

#[test]
fn a_cond_with_no_else_may_leave_the_name_nil() {
    let result =
        analyze_file("(defn pick [n] (cond (%eq n 0) (defn f [] 0) (%eq n 1) (defn f [] 1)) (f))");
    assert!(unknown(signal_of(&result, "pick")));
}

#[test]
fn an_inner_when_leaves_its_outer_branch_open_to_nil() {
    let result = analyze_file("(defn pick [c d] (if c (when d (defn f [] 1)) (defn f [] 3)) (f))");
    assert!(unknown(signal_of(&result, "pick")));
}

#[test]
fn a_loop_may_run_no_pass_and_leave_the_name_nil() {
    let result = analyze_file(
        "(defn pick [n] (def @i 0) (while (%lt i n) (defn f [] 1) (assign i (%add i 1))) (f))",
    );
    assert!(unknown(signal_of(&result, "pick")));
}

#[test]
fn functions_of_two_arities_leave_the_count_to_the_call() {
    let result = analyze_file("(defn pick [c] (if c (defn f [] 1) (defn f [x] x)) (f 1))");
    assert_eq!(signal_of(&result, "pick"), Signal::errors());
}

#[test]
fn a_keyword_collector_makes_the_call_check_its_keywords() {
    let result =
        analyze_file("(defn pick [c] (if c (defn f [&named a] a) (defn f [&named a] 1)) (f :a 1))");
    assert_eq!(signal_of(&result, "pick"), Signal::errors());
}

// A file's merge reads facts its inputs settle on only in the fixpoint's
// later passes: the first `f` reads `g`'s silent seed in Pass 2, and `g`
// yields. Facts copied when the merge closed keep the seed's silence.
#[test]
fn a_file_merge_takes_the_signal_its_inputs_settle_on() {
    let result = analyze_file(
        "(def c true)
         (defn f [] (g))
         (when c (defn f [] 1))
         (defn h [] (f))
         (defn g [] (yield 1))",
    );
    assert_eq!(signal_of(&result, "h"), Signal::yields());
}

// Both branches define `f`, so no nil reaches the call, and the first `f`,
// nested in a top-level form, reads `g`'s silent seed in Pass 2. The merge
// yields only once that lambda converges and the merge joins it again.
#[test]
fn a_file_merge_of_a_nested_lambda_def_takes_its_settled_signal() {
    let result = analyze_file(
        "(def c true)
         (if c (defn f [] (g)) (defn f [] 1))
         (defn h [] (f))
         (defn g [] (yield 1))",
    );
    assert_eq!(signal_of(&result, "h"), Signal::yields());
}
