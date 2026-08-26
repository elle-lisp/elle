//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::value::fiber::{SIG_DEBUG, SIG_IO, SIG_OK, SIG_YIELD};

#[test]
fn ok_returns_ok() {
    assert_eq!(classify(SIG_OK, &Value::NIL), SignalAction::Ok);
}

#[test]
fn resume_returns_resume() {
    assert_eq!(classify(SIG_RESUME, &Value::NIL), SignalAction::Resume);
}

#[test]
fn propagate_returns_propagate() {
    assert_eq!(
        classify(SIG_PROPAGATE, &Value::NIL),
        SignalAction::Propagate
    );
}

#[test]
fn query_returns_query() {
    assert_eq!(classify(SIG_QUERY, &Value::NIL), SignalAction::Query);
}

#[test]
fn error_returns_error() {
    assert_eq!(classify(SIG_ERROR, &Value::NIL), SignalAction::Error);
}

#[test]
fn composed_error_io_returns_error() {
    let bits = SIG_ERROR | SIG_IO;
    assert_eq!(classify(bits, &Value::NIL), SignalAction::Error);
}

#[test]
fn halt_returns_halt() {
    assert_eq!(classify(SIG_HALT, &Value::NIL), SignalAction::Halt);
}

#[test]
fn yield_returns_suspend() {
    assert_eq!(classify(SIG_YIELD, &Value::NIL), SignalAction::Suspend);
}

#[test]
fn debug_returns_suspend() {
    assert_eq!(classify(SIG_DEBUG, &Value::NIL), SignalAction::Suspend);
}

#[test]
fn user_defined_returns_suspend() {
    let user_bit = SignalBits::from_bit(32);
    assert_eq!(classify(user_bit, &Value::NIL), SignalAction::Suspend);
}

#[test]
fn abort_without_fiber_falls_through() {
    // SIG_ABORT without a fiber value should hit Error (since
    // SIG_ABORT = SIG_ERROR | SIG_TERMINAL).
    assert_eq!(classify(SIG_ABORT, &Value::NIL), SignalAction::Error);
}

#[test]
fn is_suspending_agrees_with_classify_on_every_shape() {
    // The WASM tier branches on `is_suspending` while the interpreter branches
    // on `classify`. They are separate functions, so nothing but this test stops
    // them drifting — and a drift is silent: the tier parks a caller the
    // interpreter would have let return, or vice versa.
    //
    // COUNTER-FACTUAL: `is_suspending` first read "not empty, not error, not
    // halt", which sends SIG_QUERY to Suspend where classify sends it to Query.
    // Under --wasm=full that parked the caller of `vm/config` and
    // `list-primitives` forever instead of reporting them unsupported
    // (tests/elle/{trace,unicode,vm}.lisp).
    let user_bit = SignalBits::from_bit(32);
    let shapes = [
        SIG_OK,
        SIG_ERROR,
        SIG_HALT,
        SIG_YIELD,
        SIG_IO,
        SIG_DEBUG,
        SIG_RESUME,
        SIG_PROPAGATE,
        SIG_QUERY,
        SIG_ABORT,
        user_bit,
        SIG_YIELD | SIG_IO,
        SIG_ERROR | SIG_IO,
        SIG_IO | user_bit,
        SIG_HALT | SIG_IO,
    ];
    for bits in shapes {
        let by_classify = classify(bits, &Value::NIL) == SignalAction::Suspend;
        assert_eq!(
            is_suspending(bits),
            by_classify,
            "is_suspending and classify disagree on {bits}"
        );
    }
}
