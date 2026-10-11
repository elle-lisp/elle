// audited: 2026-10-05
//! Abandoning suspended work: a call boundary, a host's refusal, and the
//! chokepoint that runs what the discarded frames still owed.
//!
//! docs/impl/region/park.md
//! docs/impl/region/owner.md
//! docs/signals/inference.md

use super::VM;
use crate::value::closure::Boundary;
use crate::value::fiber::ParamDepth;
use crate::value::{SignalBits, Value, SIG_ERROR, SIG_TERMINAL};

impl VM {
    /// Check the signal `bits` leaving a call against the callee's
    /// `boundary`. When the boundary refuses it, the refusal's error replaces
    /// the fiber's signal, and the answer is the bits the call leaves by
    /// instead. `None` when the signal crosses as it is.
    ///
    /// The park this ends is the fiber's own live signal: every site that
    /// asks here still holds it. `depth` is the parameter depth at the
    /// boundary's call ([`Self::discard_suspended_frames`]).
    pub(crate) fn enforce_boundary(
        &mut self,
        bits: SignalBits,
        boundary: &Boundary,
        depth: ParamDepth,
    ) -> Option<SignalBits> {
        let (bits, err) = self.boundary_violation(bits, boundary, self.fiber.signal, depth)?;
        self.fiber.signal = Some((bits, err));
        Some(bits)
    }

    /// The signal a call boundary raises in place of `bits`, with the
    /// suspended frames the boundary abandons discarded. `None` when `bits`
    /// cross the boundary as they are.
    ///
    /// The proof comes first. A signal that leaves a silent function and
    /// breaks its proof raises the terminal `:broken-proof` error, which no
    /// mask catches. Otherwise a signal the squelch mask names, by
    /// `signals::squelched_bits`, raises a `:signal-violation` error.
    ///
    /// The error is returned rather than stored: each site delivers it
    /// differently. The interpreter sets `fiber.signal`, and the
    /// `compile/run-on` entries return it as the call's result.
    ///
    /// `parked` is the signal whose park this boundary ends, named by the site
    /// rather than read from `fiber.signal`: two sites reach here through
    /// `invoke_closure_jit`, which restores the CALLER's signal before it asks
    /// the boundary's question and holds the parked one in a local. What the
    /// park owed is released against it (docs/impl/region/park.md). `depth` is
    /// the parameter depth at the boundary's call.
    pub(crate) fn boundary_violation(
        &mut self,
        bits: SignalBits,
        boundary: &Boundary,
        parked: Option<(SignalBits, Value)>,
        depth: ParamDepth,
    ) -> Option<(SignalBits, Value)> {
        let refusal = match &boundary.proof {
            Some(proof) if crate::signals::breaks_proof(bits) => (
                SIG_ERROR.union(SIG_TERMINAL),
                "broken-proof",
                format!(
                    "broken proof: {} is inferred silent but raised {}",
                    proof.display_label(),
                    crate::signals::registry::format_bits(bits)
                ),
            ),
            _ => {
                let squelched = crate::signals::squelched_bits(bits, boundary.squelch);
                if squelched.is_empty() {
                    return None;
                }
                (
                    SIG_ERROR,
                    "signal-violation",
                    format!(
                        "squelch: signal {} caught at boundary",
                        crate::signals::registry::format_bits(squelched)
                    ),
                )
            }
        };
        let (bits, kind, message) = refusal;
        let err = self.escaping_error(kind, message);
        // The error the boundary raises is what leaves with the signal, so it is
        // the payload the discard's own releases must leave standing — built in a
        // fresh region of its own, so in practice it exempts nothing and the
        // abandoned frames' tables run in full.
        self.discard_suspended_frames(err, parked, depth);
        Some((bits, err))
    }

    /// Discard the LIVE fiber's suspended frames (a call boundary, a host's
    /// refusal) — the chokepoint for abandoning suspended work while the fiber
    /// runs on, the discard counterpart of `resume_suspended`
    /// (docs/impl/region/owner.md). A fiber reaching a TERMINAL state instead
    /// releases through
    /// `vm::fiber::take_fiber_owned`/`release_fiber_owned`, which also frees the
    /// fiber owner node this discard leaves alone, since the fiber survives a
    /// boundary and may still own it.
    ///
    /// The discarded frames' continuations will never run, so every release that
    /// lived in one is at its last chance here. What each frame owes it carries
    /// itself, in three readings [`crate::value::fiber::ParkedDues`] collects
    /// together: its parked owner node and the releases its activation took over
    /// from its own frame-replacing tail calls (both MOVED into the frame at the
    /// suspend — the record's only home, so no completion or resume can reach
    /// them a second time), and the releases its two emitter-recorded tables name,
    /// read against its saved locals and its saved activation map.
    ///
    /// Each reading names regions this chokepoint is entitled to release: a
    /// node's members are exactly the regions the inference proved externally
    /// unique and moved in through `AdoptIntoActivation`, a deferred region is one
    /// the compiler itself named as this activation's to release, and a table
    /// entry is a release the executing function emitted for its own slot with a
    /// receipt that says it did not run. The fiber's survival is not part of that
    /// reading: a frame's saved stack and saved map were taken and cloned at its
    /// own park, and its activation returned before this point, so no live frame
    /// shares the state the receipts are read against.
    ///
    /// What stays refused is a blanket release of the rest of the frame's
    /// `activation_region_map`. It is a borrowed view carrying no receipt, and a
    /// region it names can still be live in an outer, non-discarded frame or in
    /// the activation that catches the squelch. Those regions stay leaked on this
    /// path until an ownership cut adopts them (UAF-safe, bounded per discard).
    ///
    /// A fourth reading answers for the PARK rather than for the frames, and it
    /// is the delivery ledger's: this exit is neither the reader that consumes a
    /// park's delivery retain nor the install that releases a runtime-built
    /// payload, so both are owed here (docs/impl/region/park.md).
    ///
    /// `payload` is the value the exit leaves with — a boundary's own error, or
    /// `nil` for a refusing host, which builds its error afterwards — whose
    /// region no release here may take, on the same
    /// reading the abandoned-frame walk makes (docs/impl/region/mechanism.md).
    /// `parked` is the signal whose park this ends, named by the site that
    /// ends it (see `boundary_violation`).
    ///
    /// `depth` is the parameter depth at the call the boundary or the host
    /// answers, and the fiber's parameter frames are truncated to it
    /// (src/vm/AGENTS.md).
    pub(crate) fn discard_suspended_frames(
        &mut self,
        payload: Value,
        parked: Option<(SignalBits, Value)>,
        depth: ParamDepth,
    ) {
        self.fiber.unwind_params(depth);
        if let Some(frames) = self.fiber.suspended.take() {
            let dues = crate::value::fiber::ParkedDues::of(frames);
            let protect = Some(payload).filter(|v| !self.fiber.delivery.mint_names(*v));
            let heap = unsafe { &mut *self.heap_ptr };
            crate::vm::fiber::release_parked_dues(heap, dues, protect, None);
        }
        // The park's own references run after the tables above, which still find
        // the payload's region live: a body-allocated payload's own release is
        // one of those entries, and the delivery retain dropped here is what
        // keeps it from being the region's last.
        let heap = unsafe { &mut *self.heap_ptr };
        crate::vm::fiber::release_abandoned_park(heap, &mut self.fiber.delivery, parked);
        // The abandoned park's funding has no consumer — the delivery funnel
        // that would have taken it will never run for these frames.
        self.fiber.delivery.discharge();
    }

    /// A host that runs code on the current fiber refused a park of that code.
    /// `eval`, `import`, the `compile/*-module` setup runs,
    /// `compile/run-on :jit` and the root driver cannot hold a suspension of
    /// the code they run, so each answers one with an error at its own call.
    /// The refused park has no reader and no install, and its frames never run
    /// again, so it ends as a boundary's park does: through
    /// [`Self::discard_suspended_frames`]. The host's error then parks the
    /// fiber's own frame at the host's call, so a restart answers that call
    /// (docs/impl/region/park.md).
    ///
    /// The refused signal is the one in the fiber's slot, which this takes
    /// out; the host builds its error afterwards. The refusal may release the
    /// refused payload, so a host that describes the payload reads it before
    /// it asks. `depth` is the parameter
    /// depth at the host's entry. A no-op for anything but a park (see
    /// [`Self::is_park`]).
    pub(crate) fn refuse_hosted_park(&mut self, bits: SignalBits, depth: ParamDepth) {
        if !Self::is_park(bits) {
            return;
        }
        let parked = self.fiber.signal.take();
        self.refuse_held_park(bits, parked, depth);
    }

    /// [`Self::refuse_hosted_park`] for a host that holds the refused signal
    /// `parked` itself: `invoke_closure_jit` restores the caller's signal
    /// before it asks, as it does for its boundary check.
    pub(crate) fn refuse_held_park(
        &mut self,
        bits: SignalBits,
        parked: Option<(SignalBits, Value)>,
        depth: ParamDepth,
    ) {
        if !Self::is_park(bits) {
            return;
        }
        // The host's error is built after this, in a fresh region of its own, so
        // no value leaving this exit needs the walk's payload exemption.
        self.discard_suspended_frames(Value::NIL, parked, depth);
    }

    /// A host that runs a thunk on the current fiber hands the thunk's
    /// suspension on as its own call's park: `arena/allocs` and
    /// `compile/run-on :bytecode` return the thunk's signal as their own, and
    /// the call's park is built from it. The thunk's park ends there, so its
    /// funding record must not survive into the park the host's call makes
    /// (docs/impl/region/park.md). A no-op for anything but a park (see
    /// [`Self::is_park`]).
    pub(crate) fn abandon_hosted_park(&mut self, bits: SignalBits) {
        if !Self::is_park(bits) {
            return;
        }
        self.fiber.delivery.discharge();
    }

    /// Whether `bits` leave a park a host must end: not a completion, not an
    /// error (an `:error` fiber is resumable, and its records are
    /// identity-gated), not a halt, and not the switch trampoline.
    fn is_park(bits: SignalBits) -> bool {
        use crate::value::{SIG_ERROR, SIG_HALT, SIG_SWITCH};
        !(bits.is_empty()
            || bits.intersects(SIG_ERROR)
            || bits.intersects(SIG_HALT)
            || bits == SIG_SWITCH)
    }
}

#[cfg(test)]
mod tests;
