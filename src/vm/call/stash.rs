// audited: 2026-11-13
//! A tail call's borrowed-argument stashes: the retains a signal exit consumes, and the late mints the runtime makes.
//!
//! docs/impl/region/signalexit.md
//! docs/impl/region/handoff.md

use super::handoff::CodeOffers;
use super::VM;
use crate::hir::region::RuntimeRegion;
use crate::value::arena::result_region_of;
use crate::value::Value;

/// The borrowed-argument stashes a `TailCall` carries: each one's slot, and the
/// late mints among them, whose stash holds a value the frame minted nothing
/// for until the runtime knows the callee.
#[derive(Clone, Copy)]
pub(crate) struct TailStash<'a> {
    pub(crate) slots: &'a [u16],
    pub(crate) late_mints: CodeOffers<'a>,
}

impl TailStash<'_> {
    /// A tail call with no borrowed argument: a spliced one.
    pub(crate) const NONE: TailStash<'static> = TailStash {
        slots: &[],
        late_mints: CodeOffers::NONE,
    };
}

impl VM {
    /// Stamp `nil` into each late mint's stash, before the callee is known. A
    /// closure callee mints the arguments it is handed, and a native suspend
    /// writes each back as it mints it, so every other outcome leaves the stash
    /// `nil` and the fall-through's release of it does nothing.
    pub(crate) fn stamp_late_mints(&mut self, stash: TailStash<'_>) {
        let base = self.current_frame_base();
        for offer in stash.late_mints.iter() {
            if let Some(cell) = self.fiber.stack.get_mut(base + offer.slot as usize) {
                *cell = Value::NIL;
            }
        }
    }

    /// A native tail callee suspends: make every late mint, and write each
    /// argument back into its stash. The replayed fall-through releases it, or
    /// the walk does for a park nobody resumes, as it releases a retain taken
    /// before the call.
    pub(crate) fn mint_suspended_late(&mut self, stash: TailStash<'_>, args: &[Value]) {
        let heap = unsafe { &mut *self.heap_ptr };
        // Each stash holds its minted reference from here on.
        let _ = stash.late_mints.offered().mint(heap, args);
        let base = self.current_frame_base();
        stash.late_mints.restore(args, &mut self.fiber.stack, base);
    }

    /// Resolve this tail call's borrowed-argument retains to physical regions
    /// and CONSUME the local each one is stashed in, without releasing anything
    /// yet (docs/impl/region/signalexit.md).
    ///
    /// Stamping the local `nil` is what makes the release count once: a frame
    /// that leaves by a signal can still be REPLAYED — a suspending signal parks
    /// the continuation at the post-`TailCall` ip, and an `:error` fiber is
    /// resumable for the restarts system — so the very `DecrefValueRegion` this
    /// stands in for is reached a second time, where it now loads an immediate
    /// and no-ops. The stash belongs to the block alone: it is written once
    /// before the call and read once after it, for this release and nothing
    /// else.
    ///
    /// Resolving and releasing are split so the release runs AFTER the signal
    /// handler: the handler may install the very value as the signal's payload
    /// (a fiber carrier hands over its own fiber argument) or swap the live
    /// fiber out from under the frame whose locals name it.
    ///
    /// A SUSPENDING exit takes none of them: its continuation is parked at the
    /// post-`TailCall` ip, so the block's release of each retain still runs on
    /// the resume, or off the frame's release table for a park nobody resumes.
    /// Its caller passes no slots.
    pub(crate) fn take_borrowed_arg_retains(&mut self, slots: &[u16]) -> Vec<RuntimeRegion> {
        if slots.is_empty() {
            return Vec::new();
        }
        let frame_base = self.current_frame_base();
        let heap = unsafe { &mut *self.heap_ptr };
        let mut regions = Vec::with_capacity(slots.len());
        for &slot in slots {
            let Some(cell) = self.fiber.stack.get_mut(frame_base + slot as usize) else {
                continue;
            };
            let value = *cell;
            let region = result_region_of(heap, value);
            *cell = Value::NIL;
            regions.extend(region);
        }
        regions
    }

    /// Release the regions [`Self::take_borrowed_arg_retains`] resolved. Split
    /// from the resolution so the signal handler runs between the two — see that
    /// method for why.
    pub(crate) fn run_borrowed_arg_retains(&mut self, regions: Vec<RuntimeRegion>) {
        if regions.is_empty() {
            return;
        }
        let heap = unsafe { &mut *self.heap_ptr };
        for region in regions {
            if crate::config::get().has_trace("rc") {
                eprintln!(
                    "[trace:rc] borrowed-arg retain released({region}) rc={} \
                     (tail-call signal exit)",
                    heap.region_rc(region)
                );
            }
            heap.decref_region(region);
        }
    }
}
