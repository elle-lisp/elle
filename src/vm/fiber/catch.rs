//! Does a child fiber's signal reach its parent, or does the parent's mask
//! absorb it?
//!
//! Every position that drives a child fiber — `fiber/resume` and `fiber/abort`,
//! each in call, tail, and JIT position, plus the trampoline's unwind and the
//! `SIG_SWITCH` handler — asks this one question, then differs only in how it
//! delivers the answer (operand stack, `JitValue`, or a signal return). The
//! question lives here so the rule has one definition.

use crate::value::{SignalBits, SIG_ERROR, SIG_HALT, SIG_TERMINAL};
use crate::vm::core::VM;

/// True when `mask` absorbs `bits`, so the resuming fiber receives a value
/// rather than a propagating signal.
///
/// Three cases, in order:
///
/// - A child that finished normally (`bits.is_empty()`) is always absorbed. It
///   emitted nothing, so there is nothing for a mask to miss.
/// - A terminal signal (`SIG_TERMINAL`) is never absorbed, whatever the mask
///   asks for. Terminal means the child cannot continue, so letting a parent
///   swallow the signal would leave a dead fiber that the parent believes is
///   resumable.
/// - Otherwise the mask decides, via [`SignalBits::covers`].
pub(crate) fn mask_catches(mask: SignalBits, bits: SignalBits) -> bool {
    bits.is_empty() || (mask.covers(bits) && !bits.intersects(SIG_TERMINAL))
}

impl VM {
    /// Reject a signal that nothing can ever catch, reporting a state-error
    /// attributed to `op` (the driving primitive, for example `"fiber/resume"`).
    ///
    /// A signal that escapes the child's mask travels to the resuming fiber's
    /// own parent. At the root there is no such parent, so a *resumable* signal
    /// — a yield, an I/O request — would suspend a fiber nobody will ever wake.
    /// That is a program error, and this reports it as one.
    ///
    /// `SIG_ERROR` and `SIG_HALT` are exempt: both end the program rather than
    /// waiting for a resume, so reaching the root with one is their normal
    /// destination, not an orphaning.
    ///
    /// Returns true when it reported the error, so the caller then delivers nil
    /// in whatever way its position requires.
    pub(crate) fn reject_orphaned_signal(&mut self, bits: SignalBits, op: &str) -> bool {
        let orphaned = self.current_fiber_handle.is_none()
            && !bits.intersects(SIG_ERROR)
            && !bits.intersects(SIG_HALT);
        if orphaned {
            self.set_error(
                "state-error",
                format!("{op}: cannot propagate signal (no parent fiber to catch it)"),
            );
        }
        orphaned
    }
}

#[cfg(test)]
mod tests {
    use super::mask_catches;
    use crate::value::{SIG_ERROR, SIG_IO, SIG_OK, SIG_TERMINAL, SIG_YIELD};

    #[test]
    fn a_completed_child_is_absorbed_by_any_mask() {
        // An empty mask asks to catch nothing, yet a normal return is not a
        // signal at all — there is nothing to propagate.
        assert!(mask_catches(SIG_OK, SIG_OK));
        assert!(mask_catches(SIG_YIELD, SIG_OK));
    }

    #[test]
    fn a_covered_signal_is_absorbed() {
        assert!(mask_catches(SIG_YIELD, SIG_YIELD));
        assert!(mask_catches(SIG_YIELD.union(SIG_ERROR), SIG_ERROR));
    }

    #[test]
    fn an_uncovered_signal_propagates() {
        assert!(!mask_catches(SIG_YIELD, SIG_ERROR));
        assert!(!mask_catches(SIG_OK, SIG_YIELD));
    }

    #[test]
    fn a_terminal_signal_propagates_through_a_mask_that_covers_it() {
        // The counterfactual for the terminal guard: without it, a mask naming
        // the accompanying bit would absorb a signal the child cannot resume
        // from, and the parent would go on treating a dead fiber as paused.
        let terminal = SIG_ERROR.union(SIG_TERMINAL);
        assert!(
            !mask_catches(SIG_ERROR, terminal),
            "a mask covering SIG_ERROR must not absorb a terminal error"
        );
        assert!(
            !mask_catches(SIG_ERROR.union(SIG_TERMINAL), terminal),
            "naming SIG_TERMINAL in the mask must not make it catchable either"
        );
    }

    #[test]
    fn io_requires_the_mask_to_name_io() {
        // `covers` treats SIG_IO specially: a mask must name it explicitly.
        // Pinned here because `mask_catches` delegates that rule rather than
        // restating it.
        assert!(!mask_catches(SIG_YIELD, SIG_IO));
        assert!(mask_catches(SIG_IO, SIG_IO));
    }
}
