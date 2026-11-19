// audited: 2026-11-18
//! Top-level `def` semantics, which must match the VM.
//!
//! docs/impl/wasm.md
//!
//! A file's top level binds each name from its `def` onward, as a function
//! body does, so redefining a top-level `def` makes a new binding, not an
//! error. The corpus relies on it (tests/lang/def-shadow.lisp).
//!
//! `build_full_source` branches on `has_toplevel_redefinition`. A program that
//! redefines a top-level name is restructured: definitions at the file top
//! level, and expression runs under `ev/run` (`build_scheduled_toplevel`).
//! Every other program keeps the single-thunk wrap, which runs the whole body
//! in a closure. A top-level def's value would otherwise run in the ENTRY
//! function, where `eval`'s dynamic compilation traps; in a closure it works.

use super::*;

#[test]
fn wasm_full_allows_toplevel_def_redefinition() {
    // `(def a 10)`, then `(def a (+ b 1))`, where `b` holds the first `a`. A
    // def's value cannot read its own name directly, so `b` carries the
    // previous value. This program redefines `a`, so it takes the restructure
    // path, and its defs stay at the file top level. Trailing `a` is the
    // program's value.
    assert_eq!(
        eval_with_stdlib("(def a 10)\n(def b a)\n(def a (+ b 1))\na"),
        "11",
        "a top-level def redefinition must make a new binding under \
         --wasm=full, as it does on the VM"
    );
}

// tests/impl/region-termination-sweep.lisp and
// tests/impl/region-eval-quoted-data-leak.lisp pin the single wrap's `eval`
// under `--wasm=full`: a program that redefines nothing, whose top-level def's
// value calls `eval`. No unit test here pins it, because `eval`'s wasm
// compile-context teardown segfaults the in-process test harness on drop. It is
// clean under the CLI's process exit.

#[test]
fn wasm_full_interleaved_defs_and_expression_runs_emit() {
    // Under the restructure path (this program redefines `s`), interleaved defs
    // and expressions become several `(ev/run (fn [] …))` thunks — each a
    // suspending closure — followed by a short entry. Closures emit before the
    // entry, and a suspending closure leaves resume continuations pointing into
    // ITS blocks. The entry must reset that state, or emit_cfg slices the
    // entry's own shorter block at a stale offset and panics
    // (src/wasm/controlflow.rs).
    // tests/lang/bug-propagate-free-at.lisp under --wasm=full is the file-level
    // face. The trailing (length (pairs …)) is 0.
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
