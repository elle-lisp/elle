// audited: 2026-09-29
//! The pending join a `JoinRegion` leaves on the fiber, and the next value mint
//! that consumes it.
//!
//! docs/impl/region/colocation.md

use super::*;
use crate::value::fiber::PendingJoin;
use crate::value::fiberheap::regionstore::JoinedRegion;

impl VM {
    /// Record that the next value mint for `slot` joins the region `partner`
    /// lives in. The record holds no reference, so a join nothing consumes costs
    /// nothing. A partner with no region (an immediate) leaves no join.
    pub(crate) fn set_pending_join(&mut self, slot: StaticRegion, partner: Value) {
        let heap = unsafe { &mut *self.heap_ptr };
        self.fiber.pending_join =
            crate::value::arena::region_of(heap, partner).map(|region| PendingJoin {
                slot,
                partner: MappedRegion::new(region, heap.region_generation(region.get())),
            });
    }

    /// Take the pending join if it names `slot`, and join the partner's region.
    /// A join for another slot is dropped: it was placed before a mint that never
    /// ran. `None` means the mint is fresh.
    pub(super) fn take_pending_join(&mut self, slot: StaticRegion) -> Option<JoinedRegion> {
        let pending = self.fiber.pending_join.take()?;
        if pending.slot != slot {
            return None;
        }
        self.heap()
            .join_value_region(pending.partner.region, pending.partner.gen)
    }
}

#[cfg(test)]
mod tests;
