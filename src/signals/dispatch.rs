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
/// specific primitives with known bit patterns) and `intersects()` for
/// user-facing signals (which can be composed, for example SIG_ERROR | SIG_IO).
#[inline]
pub fn classify(bits: SignalBits, value: &Value) -> SignalAction {
    if bits.is_empty() {
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
    if bits.intersects(SIG_ERROR) {
        return SignalAction::Error;
    }
    if bits.intersects(SIG_HALT) {
        return SignalAction::Halt;
    }
    SignalAction::Suspend
}

#[cfg(test)]
mod tests;
