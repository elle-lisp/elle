//! Signal classification for routing primitive return values.
//!
//! Both the VM (`vm/signal.rs`) and JIT (`jit/calls.rs`) must route
//! signal bits to the appropriate handler. This module provides a
//! single `classify` function so the routing logic is defined once.

use crate::value::fiber::{
    SignalBits, SIG_ABORT, SIG_ERROR, SIG_HALT, SIG_PROPAGATE, SIG_QUERY, SIG_RESUME,
};
use crate::value::Value;

/// Broad signal category returned by `classify`.
///
/// Each variant tells the caller *what kind* of handler to invoke;
/// the caller supplies the handler's execution semantics (stack push
/// vs JitValue return, frame saving, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAction {
    /// Normal return — push/return the value.
    Ok,
    /// Resume a suspended fiber.
    Resume,
    /// Propagate a caught signal from a child fiber.
    Propagate,
    /// Abort a fiber (graceful termination with error injection).
    Abort,
    /// VM state query (arena/allocs, vm/config-set, doc, etc.).
    Query,
    /// Error signal (may be composed with other bits like SIG_IO).
    Error,
    /// Halt the VM (graceful termination with return value).
    Halt,
    /// Suspending signal (SIG_YIELD, user-defined, SIG_DEBUG, etc.).
    Suspend,
}

/// Classify a primitive's return signal into a broad action category.
///
/// Uses exact equality for VM-internal signals (which are produced by
/// specific primitives with known bit patterns) and `contains()` for
/// user-facing signals (which can be composed, e.g. SIG_ERROR | SIG_IO).
#[inline]
pub fn classify(bits: SignalBits, value: &Value) -> SignalAction {
    if bits.is_ok() {
        return SignalAction::Ok;
    }
    if bits == SIG_RESUME {
        return SignalAction::Resume;
    }
    if bits == SIG_PROPAGATE {
        return SignalAction::Propagate;
    }
    if bits == SIG_ABORT && value.as_fiber().is_some() {
        return SignalAction::Abort;
    }
    if bits == SIG_QUERY {
        return SignalAction::Query;
    }
    if bits.contains(SIG_ERROR) {
        return SignalAction::Error;
    }
    if bits.contains(SIG_HALT) {
        return SignalAction::Halt;
    }
    SignalAction::Suspend
}

#[cfg(test)]
mod tests {
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
}
