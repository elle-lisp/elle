// audited: 2026-10-05
//! What a call boundary enforces on the signal its callee leaves by: a squelch
//! mask, and the proof a silent function carries.
//!
//! docs/signals/inference.md

use super::{Closure, ClosureTemplate};
use crate::value::fiber::SignalBits;

/// The boundary a call to a closure crosses on the way back to its caller.
///
/// A signal the squelch mask names becomes a `:signal-violation` error, and a
/// signal that breaks the proof becomes the terminal `:broken-proof` error
/// (`VM::boundary_violation`). A tail call widens the boundary of the
/// activation it replaces, so one boundary answers for a chain of them.
#[derive(Debug, Clone)]
pub struct Boundary {
    /// The signals a `squelch` or `attune` boundary blocks.
    pub squelch: SignalBits,
    /// The code object of the silent function whose proof covers the
    /// activation. A broken proof names it.
    pub proof: Option<ClosureTemplate>,
}

impl Boundary {
    /// A boundary that lets every signal through.
    pub fn none() -> Self {
        Boundary {
            squelch: SignalBits::EMPTY,
            proof: None,
        }
    }

    /// The boundary a call to `closure` crosses: the instance's squelch mask,
    /// and the proof of its function when the function is inferred silent. A
    /// polymorphic function's bits are a lower bound, so it proves nothing.
    pub fn of(closure: &Closure) -> Self {
        Boundary {
            squelch: closure.squelch_mask,
            proof: closure
                .template
                .signal()
                .is_silent()
                .then(|| (*closure.template).clone()),
        }
    }

    /// Cover a tail call that crosses `other` too. The masks join, and the
    /// first proof stands: once a silent function replaced the body, every
    /// signal that leaves the activation leaves through it.
    pub fn add(&mut self, other: Boundary) {
        self.squelch |= other.squelch;
        if self.proof.is_none() {
            self.proof = other.proof;
        }
    }
}

impl Default for Boundary {
    /// [`Boundary::none`], so a tail call's boundary can be taken out of it.
    fn default() -> Self {
        Boundary::none()
    }
}
