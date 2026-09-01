//! Behavioral correctness of self-recursion across control-flow boundaries.
//!
//! A self-recursive local function must recurse to *itself* — the same body,
//! carrying its own captured environment — regardless of what boundary the
//! recursion crosses: a yield/resume (the activation is parked and replayed), a
//! tail-call frame replacement (the activation is reused in place), or being
//! handed off as a value (passed to a higher-order call, returned, or stored,
//! then invoked). The runtime carries the executing function's identity across
//! each of these.
//!
//! These are **value** pins, not memory gauges, by necessity: a stale
//! self-reference makes the recursion silently continue as a *different* closure
//! (or with a *different* captured environment) and return a plausible
//! wrong-but-well-typed value — no region leaks and no freed page is read, so
//! neither the leak gauge nor the use-after-free oracle can see it. Only an
//! assertion on the computed value catches it. Each program below returns a
//! single integer that is a tight function of correct self-recursion, so a
//! cross-wired or stale self-reference yields a different integer. The
//! `tests/elle/recur-after-{yield,tail-call}.lisp` and `recur-as-value.lisp`
//! corpus files are the cross-tier (VM/JIT) and `--trace=guardfree` peers.
//!
//! The recursion bodies use raw `%`-ops. Call-site argument forwarding proves a
//! function's parameters only when the binding is used exclusively in callee
//! position; a `go` that is returned, stored, or passed as a value (the whole
//! point of these pins) proves its parameters with an allocation-free diverging
//! guard (`(when (%not (%int? m)) (error :m))`) instead. The final aggregations
//! run over opaque call results — fiber resumes, def-bound closures — whose
//! types the intrinsic contract cannot prove, so those sites use the stdlib
//! wrappers (`+`/`*`).

use super::*;
use crate::pipeline::compile_file_repl;

/// Compile `src` under the full stdlib and run it to completion, returning the
/// program's final value as an `i64`. Self-recursion is a stdlib-pervasive shape
/// (every variadic operator's `(letrec [go …] …)` over its varargs), so the
/// programs run on `Runtime::new()` rather than `without_stdlib()`.
fn run_int(src: &str) -> i64 {
    let mut rt = Runtime::new();
    let result = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file_repl(src, symbols, cctx, "<embed>")
            .expect("compiles")
            .0
    };
    let (vm, _symbols, cctx) = rt.parts();
    vm.execute_scheduled(&result.bytecode, cctx)
        .expect("runs")
        .as_int()
        .expect("program returns its result as an int")
}

/// recurse-after-yield: a self-recursive generator that suspends mid-recursion
/// must, on resume, continue recursing as itself even while another generator
/// running the same-shaped recursion is interleaved between its resumes. Two
/// summers capture distinct `base`s (10, 100); resuming them round-robin puts a
/// fiber switch between every pair of a single generator's resumes. Each must
/// total `4 * base` — 40 and 400 — so the encoded result `fa + 10000*fb` is
/// 4_000_040 only if each fiber resumed as *its own* `go` with *its own* `base`.
/// A self-reference cross-wired across the fiber switch (b continuing with a's
/// base, say) shifts the total off this value.
#[test]
fn self_recursion_survives_yield_resume_interleaved() {
    let src = "\
        (defn make-summer [base] \
          (fiber/new (fn [] \
            (letrec [go (fn [m acc] \
                          (if (%lt m 1) acc \
                            (begin (yield :tick) (go (%sub m 1) (%add acc base)))))] \
              (go 4 0))) |:yield|)) \
        (def a (make-summer 10)) \
        (def b (make-summer 100)) \
        (def @fa nil) (def @fb nil) (var r 0) \
        (while (%lt r 5) \
          (assign fa (fiber/resume a)) \
          (assign fb (fiber/resume b)) \
          (assign r (%add r 1))) \
        (+ fa (* 10000 fb))";
    assert_eq!(
        run_int(src),
        4_000_040,
        "two self-recursive generators interleaved across fiber switches must each \
         recurse over their own captured base (a=10 -> 40, b=100 -> 400); a result \
         other than 40 + 10000*400 means a resumed self-reference picked up the \
         wrong fiber's identity",
    );
}

/// recurse-after-tail-call: a self-recursive tail loop replaces its own
/// activation frame on every step, so the loop must keep recursing as itself —
/// same body, same captured `step` — across all those replacements. `deep-sum`
/// runs 100000 + 50000 frame-replacing tail calls accumulating captured steps of
/// 1 and 3, totalling 100000 + 150000 = 250000. A self-reference that went stale
/// after a frame replacement (wrong body, or a lost captured `step`) returns a
/// different sum.
#[test]
fn self_recursion_survives_tail_call_frame_replacement() {
    let src = "\
        (defn deep-sum [n step] \
          (letrec [go (fn [m acc] (if (%lt m 1) acc (go (%sub m 1) (%add acc step))))] \
            (go n 0))) \
        (+ (deep-sum 100000 1) (deep-sum 50000 3))";
    assert_eq!(
        run_int(src),
        250_000,
        "a self-recursive tail loop must keep recursing as itself across every frame \
         replacement, accumulating its own captured step — 100000*1 + 50000*3 = 250000",
    );
}

/// recurse-after-tail-call, identity to the base case: `go`'s base case returns
/// `go` itself — a self-reference in value position reached only after every tail
/// frame replacement of a 100000-deep descent. The returned closure must be the
/// loop's own `go`, so a second descent through it counts correctly. The program
/// returns `((descend 100000) 5)` over a counting `go`, which is 5 only if the
/// self-identity carried across all the replacements is the right closure.
#[test]
fn self_recursion_preserves_identity_to_base_case_after_tail_calls() {
    let src = "\
        (defn descend [n] \
          (letrec [go (fn [m] \
                        (when (%not (%int? m)) (error :m)) \
                        (if (%lt m 1) go (go (%sub m 1))))] \
            (go n))) \
        (defn counting [n] \
          (letrec [go (fn [m] (if (%lt m 1) 0 (%add 1 (go (%sub m 1)))))] \
            (go n))) \
        ((descend 100000) 0) \
        (counting 5)";
    assert_eq!(
        run_int(src),
        5,
        "the closure a deep tail loop yields at its base case must be its own `go`, \
         and an independent counting recursion must total 5",
    );
}

/// recurse-as-value: a self-recursive function used in value position — returned,
/// handed to a higher-order call, or stored then invoked — must materialize as
/// itself so the later invocation recurses correctly. Two `stepper`s capture
/// distinct increments (2, 5); handed off as values and each run 4 steps they
/// total 8 and 20. The encoded result `s2 + 100*s5` is 2008 only if each
/// value-position self-reference materialized its own closure with its own
/// captured increment; a shared or stale materialization cross-wires the two.
#[test]
fn self_recursion_correct_in_value_position() {
    let src = "\
        (defn make-countup [] \
          (letrec [go (fn [m] \
                        (when (%not (%int? m)) (error :m)) \
                        (if (%lt m 1) 0 (%add 1 (go (%sub m 1)))))] go)) \
        (defn stepper [inc] \
          (letrec [go (fn [m acc] \
                        (when (%not (%int? m)) (error :m)) \
                        (when (%not (%int? acc)) (error :acc)) \
                        (if (%lt m 1) acc (go (%sub m 1) (%add acc inc))))] go)) \
        (def f (make-countup)) \
        (def s2 (stepper 2)) \
        (def s5 (stepper 5)) \
        (assert (= (f 7) 7) \"returned self-recursive closure counts to 7\") \
        (+ (s2 4 0) (* 100 (s5 4 0)))";
    assert_eq!(
        run_int(src),
        2008,
        "self-recursive closures handed off as values must keep separate captured \
         increments (inc=2 over 4 -> 8, inc=5 over 4 -> 20): 8 + 100*20 = 2008",
    );
}

/// A self-recursive closure entered through the JIT's interpreter-fallback and
/// tail-call-resolution doors must recurse as itself. `compile/run-on :jit`
/// force-compiles ONLY the caller; the callee stays uncompiled and is called
/// exclusively from inside the compiled body, so the compiled caller's call
/// falls back to the interpreter — the callee's body enters through the JIT
/// helper's entry boundary, where the executing-closure register must be handed
/// across exactly as the interpreter call path hands it. A dropped handoff
/// resolves the callee's self-reference to nil and the recursion dispatches to
/// nil (a type error), so the program errors instead of returning its value.
#[cfg(feature = "jit")]
#[test]
fn self_recursion_survives_jit_to_interpreter_boundary() {
    let src = "\
        (def go \
          (letrec [g (fn [ls] (if (empty? ls) 0 (%add 1 (g (rest ls)))))] \
            g)) \
        (defn caller [xs] (+ 100 (go xs))) \
        (defn tail-caller [xs] (go xs)) \
        (def nontail (compile/run-on :jit caller (list 1 2 3))) \
        (def tail (compile/run-on :jit tail-caller (list 1 2 3 4))) \
        (+ nontail (* 1000 tail))";
    assert_eq!(
        run_int(src),
        103 + 1000 * 4,
        "a JIT-compiled caller's uncompiled self-recursive callee must recurse as \
         itself through both the interpreter-fallback (non-tail, 100+3) and the \
         tail-call-resolution (4) entry boundaries",
    );
}

/// A self-recursive closure entered through the forced bytecode tier
/// (`compile/run-on :bytecode` invokes the target's body directly, not through
/// the interpreter's own call path) must recurse as itself.
#[test]
fn self_recursion_survives_forced_bytecode_tier_entry() {
    let src = "\
        (def go \
          (letrec [g (fn [ls] (if (empty? ls) 0 (%add 1 (g (rest ls)))))] \
            g)) \
        (compile/run-on :bytecode go (list 1 2))";
    assert_eq!(
        run_int(src),
        2,
        "a self-recursive closure forced onto the bytecode tier must recurse as itself",
    );
}

/// A fiber whose body closure is ITSELF self-recursive must recurse as itself
/// on first resume — the fiber's first resume is an entry boundary into the
/// closure's body, distinct from the nested-call path the other fiber pins
/// exercise (where the self-recursive function is merely *called from* a fiber).
#[test]
fn self_recursion_as_fiber_body() {
    let src = "\
        (def f \
          (letrec [count-down (fn [m] \
                                (when (%not (%int? m)) (error :m)) \
                                (if (%lt m 1) 0 (%add 1 (count-down (%sub m 1)))))] \
            (fiber/new count-down |:error|))) \
        (fiber/resume f 3)";
    assert_eq!(
        run_int(src),
        3,
        "a self-recursive fiber body must recurse as itself on first resume",
    );
}

/// A self-recursive thunk run by `arena/allocs` (the measured-thunk entry into
/// a closure body) must recurse as itself. Zero-argument recursion carries no
/// decreasing argument, so it terminates via a mutable counter.
#[test]
fn self_recursion_as_measured_thunk() {
    let src = "\
        (def @steps 0) \
        (def sharp \
          (letrec [tick (fn [] \
                          (if (%lt steps 3) \
                            (begin (assign steps (%add steps 1)) (%add 1 (tick))) \
                            0))] \
            tick)) \
        (first (arena/allocs sharp))";
    assert_eq!(
        run_int(src),
        3,
        "a self-recursive thunk measured by arena/allocs must recurse as itself",
    );
}

/// A reference to a self-recursive binding from a **nested** lambda names the
/// enclosing binding, not the nested lambda. Inside `loop`'s body a nested `g`
/// closes over `loop` and returns it; `loop` is `g`'s SIBLING capture, never `g`'s
/// own self-edge. So `(g)` materializes `loop` (arity 2, recurses over `m`), and
/// `((g) …)` re-enters `loop`. The executing-closure mechanism resolves a
/// self-reference to the *innermost* enclosing lambda whose initializer it is — for
/// `loop`'s reference inside `g` that lambda is `g`, so `loop` there is a capture,
/// not the self of `g`. A mechanism that treated any reference to `loop` as a
/// self-edge regardless of the intervening `g` would materialize `g` (arity 0) and
/// the count call `((g) (- m 1) (+ acc 1))` would raise an arity error.
#[test]
fn self_reference_from_nested_lambda_names_the_enclosing_binding() {
    let src = "\
        (defn make [] \
          (letrec [loop (fn [m acc] \
                          (when (%not (%int? m)) (error :m)) \
                          (when (%not (%int? acc)) (error :acc)) \
                          (if (%lt m 1) acc \
                            (let [g (fn [] loop)] ((g) (%sub m 1) (%add acc 1)))))] \
            (loop 5 0))) \
        (make)";
    assert_eq!(
        run_int(src),
        5,
        "a nested lambda returning the enclosing self-recursive `loop` must yield \
         `loop` itself (arity 2), so `((g) …)` re-enters `loop` and counts 5 steps; \
         materializing the nested `g` instead would arity-error or mis-count",
    );
}
