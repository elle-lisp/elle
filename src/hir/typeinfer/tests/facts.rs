// audited: 2026-11-20
//! What ends a guard's fact: a loop's next turn, a call or an emit, a closure
//! body, and a written parameter.
//!
//! src/hir/AGENTS.md
//! docs/impl/typeinfer/facts.md
//!
//! Each program here asks what it computes. A fact that outlives the value it
//! describes lets the fold delete a branch that runs, so the wrong answer is a
//! plausible value, never a crash.

use super::{compile_result, run_booted};
use crate::value::Value;

mod declared;

/// A fact from outside a loop governs its first turn alone. The first row is
/// the report: the outer `while` proves `found` nil for its body, and the inner
/// loop writes `found` on its first turn and reads it on the next.
///
/// The counter-factual: the pass walked each loop body once, so the second
/// turn's `(nil? found)` read the outer fact, the fold decided it true, and
/// every turn wrote. The rows answered 5, 3, 3, 6 and 3.
#[test]
fn a_loop_turn_reads_what_the_last_turn_wrote() {
    for src in [
        "(let [@found nil]
           (while (nil? found)
             (each m in [1 2 5]
               (when (nil? found) (assign found m))))
           found)",
        "(let [@x nil @n 0 @hits 0]
           (while (nil? x)
             (while (< n 3)
               (when (nil? x) (assign hits (+ hits 1)))
               (assign x 1)
               (assign n (+ n 1))))
           hits)",
        "(let [@x nil @hits 0]
           (each j in [1]
             (when (nil? x)
               (each i in [1 2 3]
                 (when (nil? x) (assign hits (+ hits 1)))
                 (assign x i))))
           hits)",
        "(let [@x nil @hits 0]
           (while (nil? x)
             (each j in [1 2]
               (each i in [1 2 3]
                 (when (nil? x) (assign hits (+ hits 1)))
                 (assign x i))))
           hits)",
        "(let [@x 0 @hits 0]
           (while (int? x)
             (each i in [1 2 3]
               (when (int? x) (assign hits (+ hits 1)))
               (assign x \"s\")))
           hits)",
    ] {
        assert_eq!(run_booted(src), Value::int(1), "{src}");
    }
}

/// The arm proves `x` nil, and the call runs the closure that writes it. The
/// counter-factual: the arm's fact held over the call, the fold kept the inner
/// match's `:nil` arm alone, and the program answered `:nil`.
#[test]
fn a_call_ends_a_fact_about_a_binding_a_closure_writes() {
    let src = "(let [@x nil]
                 (def set-x (fn [] (assign x 1)))
                 (match (type-of x)
                   :nil (begin (set-x) (match (type-of x) :nil :nil _ :int))
                   _ :other))";
    assert_eq!(run_booted(src), Value::keyword("int"), "{src}");
}

/// No closure writes `x` here. The function that owns it writes it while the
/// fiber is parked at the `yield`, which is the second kind of shared binding.
/// The counter-factual: the fiber's arm fact held over the `emit`, and the
/// second resume answered `:nil`.
#[test]
fn an_emit_ends_a_fact_about_a_captured_binding_the_owner_writes() {
    let src = "(let [@x nil]
                 (def f (fiber/new
                          (fn []
                            (match (type-of x)
                              :nil (begin (yield 1)
                                          (match (type-of x) :nil :nil _ :int))
                              _ :other))
                          |:yield|))
                 (fiber/resume f)
                 (assign x 1)
                 (fiber/resume f))";
    assert_eq!(run_booted(src), Value::keyword("int"), "{src}");
}

/// A lambda written under a guard runs after the write that ends the guard's
/// value. The counter-factual: the body read the guard's nil, the fold
/// deleted its own test, and each call counted: 2, and `[true true]`.
#[test]
fn a_closure_body_reads_without_the_facts_where_it_stands() {
    let counted = "(let [@x nil @hits 0]
                     (when (nil? x)
                       (let [count-nil (fn [] (when (nil? x) (assign hits (+ hits 1))))]
                         (count-nil)
                         (assign x 1)
                         (count-nil)))
                     hits)";
    assert_eq!(run_booted(counted), Value::int(1), "{counted}");
    let escaped = "(let [@x nil @probes @[]]
                     (each i in [1 2]
                       (when (nil? x)
                         (push probes (fn [] (nil? x)))))
                     (def before ((get probes 0)))
                     (assign x 1)
                     (if (and before (not ((get probes 0)))) :right :wrong))";
    assert_eq!(run_booted(escaped), Value::keyword("right"), "{escaped}");
}

/// The read stands before the write. The counter-factual: the first pass left
/// the int the write stored, the second pass read it at the top of the
/// function, and the fold decided `(nil? x)` false.
#[test]
fn a_written_parameter_is_unknown_before_its_write() {
    let straight = "(defn fresh? [@x] (def was (nil? x)) (assign x 1) was)
                    (fresh? nil)";
    assert_eq!(run_booted(straight), Value::TRUE, "{straight}");
    let looped = "(defn count-nil [@x]
                    (var hits 0)
                    (var n 0)
                    (while (< n 3)
                      (when (nil? x) (assign hits (+ hits 1)))
                      (assign x 1)
                      (assign n (+ n 1)))
                    hits)
                  (count-nil nil)";
    assert_eq!(run_booted(looped), Value::int(1), "{looped}");
}

/// A nonzero fact ends at a call or an `emit` the same way. The
/// counter-factual: both compiled, and `%div` ran over the zero the call or
/// the owner stored, answering 0 instead of raising.
#[test]
fn a_call_or_an_emit_ends_a_nonzero_fact() {
    for src in [
        "(var d 1)
         (defn zero! [] (assign d 0))
         (defn q [] (if (%ne d 0) (begin (zero!) (%div 10 d)) 0))
         (q)",
        "(let [@d 1]
           (def f (fiber/new
                    (fn [] (when (%ne d 0) (yield 1) (%div 10 d)))
                    |:yield|))
           (fiber/resume f)
           (assign d 0)
           (fiber/resume f))",
    ] {
        let err = compile_result(src).expect_err("the divisor may be zero");
        assert!(err.contains("%div"), "{src}: {err}");
    }
}

/// A write ends a nonzero fact wherever it stands before the use: in a branch,
/// an inner sequence, a loop body, a match arm or a cond clause. The
/// counter-factual: each compiled, because the walk dropped the environment a
/// branch or a sequence left behind, and `%div` ran over the zero.
#[test]
fn a_write_before_the_use_ends_a_nonzero_fact() {
    for src in [
        "(var d 1)
         (defn q [c] (if (%ne d 0) (begin (if c (assign d 0) nil) (%div 10 d)) 0))
         (q true)",
        "(var d 1)
         (defn q [] (if (%ne d 0) (begin (begin (assign d 0) nil) (%div 10 d)) 0))
         (q)",
        "(var d 1)
         (var n 0)
         (defn q []
           (if (%ne d 0)
             (begin (while (%lt n 1) (assign d 0) (assign n 1)) (%div 10 d))
             0))
         (q)",
        "(var d 1)
         (defn q [k] (if (%ne d 0) (begin (match k :z (assign d 0) _ nil) (%div 10 d)) 0))
         (q :z)",
        "(var d 1)
         (defn q [k] (if (%ne d 0) (begin (cond (%eq k 1) (assign d 0) true nil) (%div 10 d)) 0))
         (q 1)",
    ] {
        let err = compile_result(src).expect_err("the divisor may be zero");
        assert!(err.contains("%div"), "{src}: {err}");
    }
}

/// The over-rejection guard: each rule ends a fact only where something can
/// change the value. A counter a closure writes, a loop counter, nested loop
/// counters, a guard on a binding nothing writes, a guard with no call between
/// it and its use, and a guard whose binding no code writes, across a branch
/// that calls, all keep their proofs.
#[test]
fn the_facts_that_still_hold_still_prove() {
    for src in [
        "(defn top [] (var n 0) (def inc! (fn [] (assign n (%add n 1)))) (inc!) n) (top)",
        "(defn cnt [] (var i 0) (while (%lt i 10) (assign i (%add i 1))) i) (cnt)",
        "(defn nest []
           (var t 0)
           (var i 0)
           (while (%lt i 3)
             (let [@j 0]
               (while (%lt j 3) (assign t (%add t j)) (assign j (%add j 1))))
             (assign i (%add i 1)))
           t)
         (nest)",
        "(defn adder [x] (when (%int? x) (fn [] (%add x 1)))) ((adder 5))",
        "(defn q [d] (if (%ne d 0) (%div 10 d) 0)) (q 2)",
        "(var d 1)
         (defn q [c] (if (%ne d 0) (begin (if c (length [1]) nil) (%div 10 d)) 0))
         (q true)",
    ] {
        compile_result(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
    }
}
