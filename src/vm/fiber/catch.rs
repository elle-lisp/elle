//! Does a child fiber's signal reach its parent, or does the parent's mask
//! absorb it?
//!
//! Every position that drives a child fiber — `fiber/resume` and `fiber/abort`,
//! each in call, tail, and JIT position, plus the trampoline's unwind and the
//! `SIG_SWITCH` handler — asks this one question, then differs only in how it
//! delivers the answer (operand stack, `JitValue`, or a signal return). The
//! question lives here so the rule has one definition.

use crate::value::{SignalBits, Value, SIG_ERROR, SIG_HALT, SIG_TERMINAL};
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
    /// [`mask_catches`] for `child`'s signal, plus the state change catching an
    /// error implies.
    ///
    /// Absorbing `SIG_ERROR` ends that error's propagation, so the location the
    /// dispatch loop recorded for it stops answering "where did the error now
    /// travelling come from?". It parks on `child` paired with `value`, the
    /// payload it describes, and `fiber/propagate` reads it back if `child`'s
    /// signal is re-raised (docs/impl/vm.md § "Where a reported error's
    /// location comes from"). Either way the live record is cleared: it is
    /// first-writer-wins (`VM::record_error_loc`), so a location left standing
    /// past its own error would be handed to the next one instead of letting it
    /// record its own.
    ///
    /// Every position that drives a child fiber asks this rather than
    /// [`mask_catches`] directly. The exception is a *lookahead* — asking
    /// whether some other fiber's signal will be caught, without catching it
    /// here — which must leave a still-propagating error's record alone.
    pub(crate) fn absorbs(
        &mut self,
        child: &crate::value::FiberHandle,
        mask: SignalBits,
        bits: SignalBits,
        value: Value,
    ) -> bool {
        let caught = mask_catches(mask, bits);
        if caught && bits.intersects(SIG_ERROR) {
            if let Some(loc) = self.error_loc.take() {
                child.with_mut(|f| f.error_loc = Some((value, loc)));
            }
        }
        caught
    }

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
    fn an_io_request_is_caught_by_any_bit_it_names() {
        // No bit is privileged: `mask_catches` delegates to `covers`, which is
        // plain overlap. A `|:yield|` mask still misses an I/O request, but
        // because the two share no bit — not because of a rule about `:io`.
        assert!(mask_catches(SIG_IO, SIG_IO));
        assert!(!mask_catches(SIG_YIELD, SIG_IO));
    }
}
