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

/// True when `bits` parks its caller rather than returning, unwinding, or
/// asking the VM to do something.
///
/// The question every tier asks after a call, and it must have one answer. The
/// interpreter asks it in `VM::call_inner`; the WASM tier asks it in `rt_call`,
/// where the result becomes the `suspended` word emitted code branches on.
///
/// Deliberately not a test for any particular bit. `:yield`, `:io`, `:wait`,
/// `:fuel`, and every user-defined signal all park, and a compound signal parks
/// on the strength of any of them — so the rule is stated by exclusion.
///
/// The VM-internal dispatch signals are excluded because they are requests to
/// the VM, not suspensions: `SIG_QUERY` asks it to read fiber state, `SIG_RESUME`
/// and `SIG_PROPAGATE` to drive a child fiber. Treating one as a park makes a
/// caller wait for a resume nobody will deliver. `SIG_ABORT` needs no case of
/// its own — it is `SIG_ERROR | SIG_TERMINAL`, so the error test already covers
/// it. `dispatch/tests.rs` pins that this agrees with [`classify`] bit for bit.
#[inline]
pub fn is_suspending(bits: SignalBits) -> bool {
    if bits.is_empty() || bits == SIG_RESUME || bits == SIG_PROPAGATE || bits == SIG_QUERY {
        return false;
    }
    !bits.intersects(SIG_ERROR) && !bits.intersects(SIG_HALT)
}

#[cfg(test)]
mod tests;
