// audited: 2026-11-23
//! What a call boundary enforces on the signal its callee leaves by: a squelch
//! mask, and the proof a silent function carries. A call reads its parts first.
//!
//! docs/signals/inference.md
//! docs/signals/squelch.md
//! docs/signals/jit.md

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

    /// The boundary a call to `closure` crosses, built from its parts at once.
    pub fn of(closure: &Closure) -> Self {
        BoundaryParts::of(closure).boundary()
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

/// What a call reads from a closure before it runs, so that it can build the
/// closure's boundary after the call without reading the closure again
/// (docs/signals/jit.md). It holds the squelch mask and a copy of the code
/// object's payload slice, and no reference to the closure. The payload stays
/// readable while its code region lives, which the JIT cache's pin assures for
/// compiled code.
#[derive(Debug, Clone)]
pub struct BoundaryParts {
    squelch: SignalBits,
    template: ClosureTemplate,
}

impl BoundaryParts {
    /// The parts of `closure`'s boundary.
    #[inline]
    pub fn of(closure: &Closure) -> Self {
        BoundaryParts {
            squelch: closure.squelch_mask,
            template: (*closure.template).clone(),
        }
    }

    /// The boundary these parts make: the squelch mask, and the function as
    /// the proof when it is inferred silent. A polymorphic function's bits are
    /// a lower bound, so it proves nothing.
    pub fn boundary(&self) -> Boundary {
        Boundary {
            squelch: self.squelch,
            proof: self
                .template
                .signal()
                .is_silent()
                .then(|| self.template.clone()),
        }
    }
}

impl Default for Boundary {
    /// [`Boundary::none`], so a tail call's boundary can be taken out of it.
    fn default() -> Self {
        Boundary::none()
    }
}
