// audited: 2026-11-16
//! What a `squelch` or `attune` boundary blocks: at run time, and in the signal the compiler gives a binding of one.
//!
//! docs/signals/squelch.md

use super::{Signal, CAP_MASK, SIG_ERROR, SIG_HALT, SIG_PAUSE, SIG_SWITCH};
use crate::value::fiber::SignalBits;

/// The bits every boundary lets out: a signal that carries one passes whole,
/// whatever else it carries. `:error` and `:halt` are the escapes, and a pause
/// bit is the VM's own, raised under whatever code was running.
pub const BOUNDARY_ESCAPES: SignalBits = SIG_ERROR.union(SIG_HALT).union(SIG_PAUSE);

/// The mask `(attune permitted f)` puts on `f`: every bit a program can raise
/// but the permitted ones. The primitive, the analyzer and the solver each
/// build it here.
pub const fn attune_mask(permitted: SignalBits) -> SignalBits {
    CAP_MASK.subtract(permitted)
}

/// The bits a `squelch`/`attune` boundary converts into a `signal-violation`
/// when a closure carrying `mask` produces `bits`. An empty result means the
/// signal crosses the boundary untouched.
///
/// One method asks it, `VM::boundary_violation`, which every tier's call,
/// tail-call and sentinel paths reach, so the exemptions cannot drift apart
/// between tiers.
///
/// Three classes never violate a boundary:
///
/// - A signal carrying one of the [`BOUNDARY_ESCAPES`]. A dynamic breakpoint's
///   `|:debug :fuel|` must reach the debugger through a squelch of `:debug`.
/// - `:switch`, matched exactly, is the VM's fiber-switch trampoline. The exact
///   match keeps a user signal that merely rides alongside enforceable.
#[inline]
pub fn squelched_bits(bits: SignalBits, mask: SignalBits) -> SignalBits {
    if bits.intersects(BOUNDARY_ESCAPES) || bits == SIG_SWITCH {
        return SignalBits::EMPTY;
    }
    bits.intersection(mask)
}

impl Signal {
    /// The signal of a closure whose boundary carries `mask`: the bits the mask
    /// blocks go, and `SIG_ERROR` comes in, because the boundary turns each into
    /// a `:signal-violation` error. The mask blocks none of the
    /// [`BOUNDARY_ESCAPES`]. When it blocks nothing this signal holds, the
    /// signal is unchanged. The analyzer asks it of a binding of a literal-mask
    /// boundary, and `Closure::effective_signal` of a closure's own mask.
    pub const fn squelch(self, mask: SignalBits) -> Signal {
        let blocked = self.bits.intersection(mask.subtract(BOUNDARY_ESCAPES));
        if blocked.is_empty() {
            return self;
        }
        Signal {
            bits: self.bits.subtract(blocked).union(SIG_ERROR),
            propagates: self.propagates,
        }
    }
}
