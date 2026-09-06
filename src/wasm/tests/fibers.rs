// audited: 2026-09-06
// docs/impl/wasm.md
//! Suspension that crosses the host: the scheduler, and `protect`/`defer`
//! around a body that parks.
//!
//! A fiber whose body suspends is driven by the host resume chain, and the
//! nested fiber `protect` and `defer` build must be re-driven when its own
//! suspension propagates through the resumer — see src/wasm/AGENTS.md §
//! "Uncaught-suspend propagation + re-drive". Each test below is a shape that
//! returned a wrong value — not an error — when one of those handoffs was
//! missing.

use super::*;

#[test]
fn wasm_full_tail_io_in_fiber_delivers_result() {
    // A fiber whose final action is a tail-position io (`ev/sleep`) must have
    // the io submitted by the scheduler and complete, not be re-queued and
    // resumed with a stale nil. `rt_prepare_tail_call` writes the native's
    // SIG_IO to memory; `handle_wasm_result` must OR (not replace) SIG_YIELD so
    // the scheduler still sees SIG_IO on fiber/bits. `ev/sleep` completes nil.
    assert_eq!(
        eval_with_stdlib("(ev/join (ev/spawn (fn [] (ev/sleep 0.001))))"),
        "nil",
        "a fiber ending in a tail-position io must complete under --wasm=full"
    );
}

#[test]
fn wasm_full_wait_via_call_resumes_continuation() {
    // A fiber that suspends on a structured-concurrency wait THROUGH a function
    // call (`ev/join`, whose signal narrows to SIG_WAIT without SIG_YIELD) must
    // resume into the code after the call. The LIR must mark a SIG_WAIT call as
    // suspending (a CPS continuation frame); keyed off SIG_YIELD alone, the
    // continuation `(+ x 2)` was dropped and the fiber returned the wait result.
    assert_eq!(
        eval_with_stdlib(
            "(ev/join (ev/spawn (fn [] (let [x (ev/join (ev/spawn (fn [] 1)))] (+ x 2)))))"
        ),
        "3",
        "a wait-via-call must resume its continuation under --wasm=full"
    );
}

#[test]
fn wasm_full_scheduler_resumes_joined_fiber() {
    // The motivating case: `ev/join` emits a `:wait` struct the compiled
    // scheduler dispatches with struct application; the child's value must flow
    // back through `handle-join` to the joiner. Before the collection fallback,
    // `handle-wait`'s `(request :op)` errored and the join never resumed.
    assert_eq!(
        eval_with_stdlib("(ev/join (ev/spawn (fn () 42)))"),
        "42",
        "the async scheduler must resume a joined fiber under --wasm=full — its \
         request dispatch is built on struct-as-function application"
    );
}

#[test]
fn wasm_full_protect_around_suspending_join_returns_value() {
    // `protect` wraps its body in a nested fiber `f` with mask SIG_ERROR only and
    // resumes it once. When the body suspends on a scheduler `:wait` (an inner
    // `ev/join`), `f` emits SIG_WAIT, which `f`'s mask does not cover. The host
    // resume path must PROPAGATE that uncaught wait through the resumer (so the
    // scheduler catches it) and RE-DRIVE `f` on the resumer's resume, so the
    // single `(fiber/resume f)` only returns once `f` completes. Parking `f` and
    // returning signal 0 (the old behavior) made `protect` read the raw
    // wait-request struct instead of the body's value.
    assert_eq!(
        eval_with_stdlib("(protect (ev/join (ev/spawn (fn [] (+ 10 20)))))"),
        "[true 30]",
        "protect around a suspending join must return [true value] under --wasm=full"
    );
}

#[test]
fn wasm_full_protect_around_suspending_join_captures_error() {
    // The child errors; `ev/join` re-raises it inside `f`, whose mask catches
    // SIG_ERROR, so `f` ends non-`:dead` holding the error. `protect` reports
    // `[false error]`. Before the fix the wait never propagated, so `protect`
    // returned the raw `:join` request as if it were the error payload.
    assert_eq!(
        eval_with_stdlib("(protect (ev/join (ev/spawn (fn [] (error {:e 1})))))"),
        "[false {:e 1}]",
        "protect around a suspending join must capture the child's error under --wasm=full"
    );
}

#[test]
fn wasm_full_protect_around_direct_error_unaffected() {
    // Regression guard: a direct (non-suspending) error inside `protect` is
    // caught by `f`'s mask exactly as before — the suspend-propagation fix must
    // not disturb the already-correct error path.
    assert_eq!(
        eval_with_stdlib("(protect (error {:e 2}))"),
        "[false {:e 2}]",
        "protect around a direct error must still return [false error] under --wasm=full"
    );
}

#[test]
fn wasm_full_defer_around_suspending_body_runs_cleanup() {
    // `defer` shares `protect`'s nested-fiber machinery: it resumes `f` once,
    // then runs cleanup, then returns the body's value (or propagates its error).
    // The cleanup must run and the suspending body's value must flow through.
    assert_eq!(
        eval_with_stdlib(
            "(let [log @[]] \
               (let [v (defer (push log :cleaned) \
                         (ev/join (ev/spawn (fn [] 42))))] \
                 [v log]))"
        ),
        "[42 @[:cleaned]]",
        "defer around a suspending body must return its value and run cleanup under --wasm=full"
    );
}
