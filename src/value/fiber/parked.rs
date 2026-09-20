// audited: 2026-09-19
//! What a fiber that can never run again strands: the releases its parked frames
//! still owe, and the retain its parked signal took.
//!
//! docs/impl/region/park.md
//! docs/impl/region/mechanism.md

use super::{Fiber, SignalBits, SuspendedFrame};
use crate::value::Value;

/// Everything the activations of a parked frame chain still owe, read off the
/// frames themselves.
///
/// One reading serves every site that abandons a chain — the squelch/abort
/// discard chokepoint (`VM::discard_suspended_frames`), the terminal-fiber
/// teardown, and the region free path's fiber discharge — because what makes a
/// release owed is a property of the FRAME, not of the fiber: the frame's saved
/// stack and saved activation map were taken and cloned at its own park, and its
/// activation returned before any of those sites was reached. So a fiber that
/// survives the abandonment (the squelch case) changes nothing here.
#[derive(Default)]
pub struct ParkedDues {
    /// Each `BytecodeFrame`'s activation owner node, in chain order.
    pub nodes: Vec<crate::hir::region::RuntimeRegion>,
    /// The releases each `BytecodeFrame`'s activation took over from its own
    /// frame-replacing tail calls, in chain order. Kept apart from
    /// [`Self::nodes`] because the two are freed differently: a node's members are
    /// gathered under the fiber node before its subtree drop, while a deferred
    /// region is `Counted` throughout and takes the plain decref its emitting
    /// instruction never ran (docs/impl/region/owner.md § "A deferred tail-call
    /// release has the node's life").
    pub deferred: Vec<crate::hir::region::RuntimeRegion>,
    /// The values each `BytecodeFrame` owes a release for — read out of its saved
    /// locals at the slots its own `Code::frame_release_slots` names. A frame
    /// nothing can re-enter never reaches the `LoadLocal s; DecrefValueRegion;
    /// StoreLocal s nil` route that would have released them, so the one release
    /// each is owed runs at the abandonment.
    ///
    /// This is the compiler's own release table, not the activation map: a mapped
    /// slot can be stale, which is why the map contributes only through
    /// [`Self::owed_regions`] below, while a value-route slot carries its own
    /// receipt — the route stamps it nil, so a slot still holding a heap value is
    /// a release that did not run.
    pub owed: Vec<Value>,
    /// The same for the frames' **slot-routed** releases: a static region slot
    /// still mapped in a parked activation is a `DecrefRegion` that did not run.
    /// Carried with its establishing generation so a consumer can tell a live
    /// mapping from a leftover the frame's own release already answered for.
    pub owed_regions: Vec<crate::hir::region::MappedRegion>,
}

impl ParkedDues {
    /// Read a parked chain, innermost frame first. A `FiberResume` frame owes
    /// nothing — its sub-fiber has a lifecycle of its own.
    pub fn of(frames: impl IntoIterator<Item = SuspendedFrame>) -> Self {
        let mut dues = ParkedDues::default();
        for frame in frames {
            let SuspendedFrame::Bytecode(f) = frame else {
                continue;
            };
            dues.nodes.extend(f.activation_dues.owner_node);
            dues.deferred
                .extend(f.activation_dues.deferred.iter().copied());
            // The releases this frame still owed. Its locals sit at the base of
            // the saved stack (the activation's own frame base, the stack having
            // been emptied at entry), so the emitter's slot indexes address them
            // directly.
            for slot in f.code.frame_release_slots().iter() {
                match f.stack.get(*slot as usize) {
                    Some(v) if v.as_heap_ptr().is_some() => dues.owed.push(*v),
                    _ => {}
                }
            }
            // The slot-routed half: a static region slot still mapped in the
            // parked activation is a `DecrefRegion` that did not run, the release
            // having taken the mapping wherever it did. Named by the frame's own
            // function so a caller's leftovers past a tail call stay out.
            for slot in f.code.frame_release_regions().iter() {
                if let Some(m) = f.activation_region_map.get(slot) {
                    dues.owed_regions.push(*m);
                }
            }
        }
        dues
    }
}

/// The parked region state a fiber that can never run again strands, TAKEN out
/// of the fiber so exactly one release path reaches it: everything its parked
/// frames owe ([`ParkedDues`]), and the park escape retain on the parked
/// signal's value (otherwise released only on the resume path). Consumed by the
/// terminal-fiber teardown (`vm::fiber::release_fiber_owned`) and by the region
/// free path's fiber discharge (`RegionStore::teardown_set`).
pub struct ParkedState {
    /// What the parked frames owed.
    pub dues: ParkedDues,
    /// The parked NON-TERMINAL signal (a yielded value, a yielding io request, a
    /// capability-denial payload) — its park took exactly one escape retain
    /// (`EmitEscape` / `SuspendEscape`) whose symmetric release lives on the
    /// resume path the fiber will never take.
    ///
    /// `None` when the parked signal is TERMINAL. A terminal signal keeps its
    /// slot for `fiber/value`, and the one retain pinning it — the park retain
    /// (`incref_signal_region`) — is the free-time signal scan's to release.
    /// Reporting it here is an over-free rather than a second discharge: a
    /// terminal signal reaches the slot by paths that take no escape retain at
    /// all (a native error's `set_error`, a bare `Return`). What a terminal EMIT
    /// owes is settled through [`Self::protect`] instead.
    pub signal: Option<(SignalBits, Value)>,
    /// The value the fiber's signal carries, if any — the payload a discharge
    /// must leave standing. A consumer skips a [`ParkedDues::owed`] entry living
    /// in this value's region: a terminal payload is the fiber's result and a
    /// non-terminal one is the [`Self::signal`] discharge's own, and a frame may
    /// well hold the very value the payload names.
    ///
    /// `None` also where the raise MINTED the payload's delivery itself (the
    /// ledger's `mint_names` answers for the live signal): the frame's reference
    /// funds nothing there, so the owed-release tables run in full and the one
    /// reference the raise chain held is reclaimed rather than stranded.
    pub protect: Option<Value>,
}

impl Fiber {
    /// Take the parked region state of a fiber that can never run again — see
    /// [`ParkedState`]. Empties the fiber's `suspended` chain and, for a parked
    /// non-terminal signal this fiber OWNS, the `signal` slot, so no second
    /// release path can reach them.
    ///
    /// Ownership of the signal's park escape retain is read from the chain's
    /// innermost frame: a `Bytecode` frame means the suspend ran here (the
    /// retain was taken with this park); a `FiberResume` frame means the signal
    /// is a propagated VIEW of an awaited child's park — the child owns the
    /// retain, and releasing the view too would double-free the one retain
    /// across two discharges.
    pub fn take_parked_state(&mut self) -> ParkedState {
        let frames = self.suspended.take().unwrap_or_default();
        let owns_signal = matches!(frames.first(), Some(SuspendedFrame::Bytecode(_)));
        let dues = ParkedDues::of(frames);
        let signal = match self.signal {
            Some((bits, _)) if owns_signal && !crate::vm::fiber::is_terminal_signal(bits) => {
                self.signal.take()
            }
            _ => None,
        };
        // The signal's payload leaves with the fiber's result — read through
        // `fiber/value`, or accounted by the signal discharge below — so a slot
        // naming its region is not this discharge's to release. Reported rather
        // than filtered here: the region behind a value is the heap's to resolve,
        // and both consumers have one. An emit-minted error payload is the
        // exception: its delivery was retained at the raise, so the frames'
        // owed releases run in full (see `protect`'s doc).
        let protect = signal
            .or(self.signal)
            .map(|(_, v)| v)
            .filter(|v| !self.delivery.mint_names(*v));
        // A discharged park is over, and the discharge below already runs its one
        // decref — so no funding record survives to a later resume of this fiber
        // (a hard kill leaves an `:error` fiber resumable, and it must find
        // nothing to mint or release).
        if signal.is_some() {
            self.delivery.discharge();
        }
        ParkedState {
            dues,
            signal,
            protect,
        }
    }
}
