// audited: 2026-09-10
// docs/impl/region/owner.md
// docs/impl/region/mechanism.md
//! Abandoning suspended work: the squelch boundary that raises a
//! signal-violation, and the one chokepoint that runs what the discarded frames
//! still owed.

use super::VM;
use crate::value::{SignalBits, Value};

impl VM {
    /// Check a signal against a squelch mask. If the signal is squelched,
    /// sets the fiber to a signal-violation error and returns `true`.
    /// Callers handle any additional side effects (stack push, call_stack pop, etc.).
    ///
    /// Which bits a boundary enforces is `signals::squelched_bits`' answer, shared
    /// with the JIT's inlined checks.
    pub(crate) fn enforce_squelch(&mut self, bits: SignalBits, mask: SignalBits) -> bool {
        let squelched = crate::signals::squelched_bits(bits, mask);
        if squelched.is_empty() {
            return false;
        }
        // The park this boundary ends is the fiber's own live signal here: every
        // site reaching the boundary through this predicate is still holding it
        // (the two that are not pass their own — see `squelch_violation`).
        let err = self.squelch_violation(squelched, self.fiber.signal);
        self.fiber.signal = Some((crate::value::SIG_ERROR, err));
        true
    }

    /// Build the `signal-violation` error a squelch/attune boundary raises for
    /// `squelched`, and discard the suspended frames the boundary abandons.
    ///
    /// The error value is returned rather than stored: each enforcement site
    /// delivers it differently — the interpreter sets `fiber.signal`, the
    /// `compile/run-on` entry returns it as the call's result. `squelched` must
    /// be non-empty, the answer `signals::squelched_bits` gives.
    ///
    /// `parked` is the signal whose park this boundary ends, named by the site
    /// rather than read from `fiber.signal`: two sites reach here through
    /// `invoke_closure_jit`, which restores the CALLER's signal before it asks
    /// the boundary's question and holds the parked one in a local. What the
    /// park owed is released against it (docs/impl/region/owner.md § "A boundary
    /// ends a park with no reader and no install").
    pub(crate) fn squelch_violation(
        &mut self,
        squelched: SignalBits,
        parked: Option<(SignalBits, Value)>,
    ) -> Value {
        let squelched_str = crate::signals::registry::format_bits(squelched);
        let err = self.escaping_error(
            "signal-violation",
            format!("squelch: signal {} caught at boundary", squelched_str),
        );
        // The error the boundary raises is what leaves with the signal, so it is
        // the payload the discard's own releases must leave standing — built in a
        // fresh region of its own, so in practice it exempts nothing and the
        // abandoned frames' tables run in full.
        self.discard_suspended_frames(err, parked);
        err
    }

    /// Discard the LIVE fiber's suspended frames (squelch / abort) — the
    /// chokepoint for abandoning suspended work while the fiber runs on, the
    /// discard counterpart of `resume_suspended` (docs/impl/region/owner.md
    /// § "A discard runs what the abandoned frames owed"; a fiber reaching a
    /// TERMINAL state instead releases through
    /// `vm::fiber::take_fiber_owned`/`release_fiber_owned`, which also frees the
    /// fiber owner node this discard leaves alone — the fiber survives a squelch
    /// and may still own it).
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
    /// payload, so both are owed here (docs/impl/region/owner.md § "A boundary
    /// ends a park with no reader and no install").
    ///
    /// `payload` is the value the exit leaves with — the boundary's own
    /// `signal-violation` error — whose region no release here may take, on the
    /// same reading the abandoned-frame walk makes
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). `parked` is the signal whose park this ends, named by the
    /// enforcement site (see `squelch_violation`).
    pub(crate) fn discard_suspended_frames(
        &mut self,
        payload: Value,
        parked: Option<(SignalBits, Value)>,
    ) {
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

    /// A host that drives a thunk on the CURRENT fiber (`eval`, `import`,
    /// `arena/allocs`, `compile/run-on`, the root driver) refuses a
    /// suspend-class signal it cannot host: it extracts the signal as a value
    /// or reports it, and the fiber runs on. The park that raised the signal
    /// is dead at that moment, so its funding record must not survive into
    /// the fiber's next park — the delivery funnel that would consume it
    /// belongs to a resume no host will ever run (docs/impl/region/owner.md
    /// § "A park names its funding in the delivery ledger"). A no-op for a
    /// completion, an error (an `:error` fiber is resumable and its records
    /// are identity-gated), a halt, or the switch trampoline — none of those
    /// abandons a suspend-class park.
    pub(crate) fn abandon_hosted_park(&mut self, bits: SignalBits) {
        use crate::value::{SIG_ERROR, SIG_HALT, SIG_SWITCH};
        if bits.is_empty()
            || bits.intersects(SIG_ERROR)
            || bits.intersects(SIG_HALT)
            || bits == SIG_SWITCH
        {
            return;
        }
        self.fiber.delivery.discharge();
    }
}
