// audited: 2026-09-06
// docs/impl/wasm.md
//! Top-level `def` semantics, which must match the VM.
//!
//! A file's top level uses sequential shadowing, so redefining a top-level
//! `def` is a redefinition (the RHS sees the previous binding), not an error —
//! a language feature the corpus relies on (tests/elle/def-shadow.lisp). The
//! naive full-module wrap put the whole user body in one `(fn [] …)`, making
//! those defs a fn-body letrec* where a duplicate binding is rejected, so
//! def-shadow/numeric/… failed to compile under `--wasm=full`.
//!
//! `build_full_source` now branches on `has_toplevel_redefinition`: a program
//! that redefines a top-level name is restructured (definitions at the file top
//! level, expression runs under `ev/run`; `build_scheduled_toplevel`) to match
//! the VM; every other program keeps the single-thunk wrap, which preserves
//! closure execution context for the whole body — needed because a top-level
//! def's RHS runs in the ENTRY function, where `eval`'s dynamic compilation
//! traps while working in a closure.

use super::*;

#[test]
fn wasm_full_allows_toplevel_def_redefinition() {
    // `(def a 10)` then `(def a (+ a 1))` — the redefinition's RHS reads the
    // previous `a`. Rejected as a duplicate binding when nested in a thunk;
    // allowed at the file top level. This program redefines `a`, so it takes the
    // restructure path. Trailing `a` is the program's value.
    assert_eq!(
        eval_with_stdlib("(def a 10)\n(def a (+ a 1))\na"),
        "11",
        "top-level def redefinition must use sequential shadowing under \
         --wasm=full, as it does on the VM"
    );
}

// The single-wrap-preserves-`eval` behavior (a non-redefining program whose
// top-level def RHS calls `eval`, which traps in the entry function but works in
// a closure) is pinned by tests/elle/region-termination-sweep.lisp and
// tests/elle/region-eval-quoted-data-leak.lisp under `--wasm=full`, not a unit
// test here: `eval`'s wasm compile-context teardown segfaults the in-process
// test harness on drop, though it is clean under the CLI's process exit.

#[test]
fn wasm_full_interleaved_defs_and_expression_runs_emit() {
    // Under the restructure path (this program redefines `s`), interleaved defs
    // and expressions become several `(ev/run (fn [] …))` thunks — each a
    // suspending closure — followed by a short entry. Closures emit before the
    // entry, and a suspending closure leaves resume continuations pointing into
    // ITS blocks; the entry must reset that state or emit_cfg slices the entry's
    // own shorter block at a stale offset and panics (src/wasm/controlflow.rs,
    // was tests/elle/bug-propagate-free-at.lisp under --wasm=full). The trailing
    // (length (pairs …)) is 0 — an immediate, safe to return per `eval`'s caveat.
    assert_eq!(
        eval_with_stdlib(
            "(def s @{:a (or nil {})})\n\
             (assert (= (type-of (get s :a)) :struct) \"a\")\n\
             (def s @{:a (or nil {})})\n\
             (assert (= (type-of (get s :a)) :struct) \"b\")\n\
             (def s @{:a (or nil {})})\n\
             (assert (= (type-of (get s :a)) :struct) \"c\")\n\
             (println (type-of (get s :a)))\n\
             (println (type-of (get s :a)))\n\
             (println (type-of (get s :a)))\n\
             (println (type-of (get s :a)))\n\
             (println (type-of (get s :a)))\n\
             (println (type-of (get s :a)))\n\
             (length (pairs (get s :a)))"
        ),
        "0",
        "interleaved defs and expression runs must emit without carrying stale \
         resume continuations into the entry function"
    );
}

#[test]
fn wasm_full_toplevel_defs_still_run_under_scheduler() {
    // On the restructure path (this program redefines `n`), keeping defs
    // top-level must not lose the scheduler. `sys/join`'s deadline is
    // scheduler-cooperative (chan/select) and fails outside `ev/run` — the exact
    // reason concurrency.lisp needs the wrap — so a passing join proves the
    // expression run executes under the scheduler. The redefined top-level `n` is
    // read inside that same expression, proving defs stay visible. Sum is 21.
    assert_eq!(
        eval_with_stdlib("(def n 5)\n(def n 20)\n(+ n (sys/join (sys/spawn-vm (fn () 1))))"),
        "21",
        "expression runs must still execute under ev/run (so scheduler-dependent \
         sys/join works) while top-level defs remain visible to them"
    );
}
