// audited: 2026-10-06
//! The releases a live interpreter frame still owed when a boundary ends it without parking it.
//!
//! docs/impl/region/unwind.md
//! docs/impl/region/mechanism.md

use super::*;

impl VM {
    /// Run the releases the running activation still owed, off the two tables
    /// the emitter recorded on `code` (docs/impl/region/unwind.md).
    ///
    /// An error parks its frame, so no raise ends one here. The caller is
    /// `end_refused_activation`: a squelch boundary refused the activation's
    /// tail call, and the refused code left nothing parked to discard, so this
    /// frame will never run again. Its locals are still on the fiber stack above
    /// the frame base.
    ///
    /// A value route is `LoadLocal s; DecrefValueRegion; StoreLocal s nil`, so a
    /// slot still holding a heap value is a release that did not run, and the
    /// walk runs it and stamps the slot nil as the route would. A slot route's
    /// receipt is the activation map, read by
    /// [`Self::release_abandoned_regions`].
    ///
    /// `payload` is the value leaving with the signal. Where the raise did NOT
    /// mint the delivery, the frame's reference IS the delivery, so a slot naming
    /// the payload's region is skipped and its release stays owed. A recorded
    /// mint (the ledger's `record_mint`) exempts nothing.
    pub(crate) fn release_abandoned_frame(&mut self, code: &crate::value::Code, payload: Value) {
        let base = self.current_frame_base();
        let slots = code.frame_release_slots().to_vec();
        let regions = code.frame_release_regions().to_vec();
        self.release_abandoned_regions(&regions, payload);
        if slots.is_empty() {
            return;
        }
        let heap = unsafe { &mut *self.heap_ptr };
        let payload_region = self.unminted_payload_region(heap, payload);
        for slot in slots {
            let Some(value) = self.fiber.stack.get(base + slot as usize).copied() else {
                continue;
            };
            let heap = unsafe { &mut *self.heap_ptr };
            let Some(region) = crate::value::arena::result_region_of(heap, value) else {
                continue;
            };
            if payload_region == Some(region) {
                continue;
            }
            self.fiber.stack[base + slot as usize] = Value::NIL;
            Self::freelog_abandoned("value route", slot as u32, region);
            self.heap().decref_region(region);
        }
    }

    /// The payload region the walk must leave standing, or `None` when nothing
    /// is exempt: an immediate payload has no region, and an emit-raised error
    /// minted its own delivery (the ledger's `mint_names` matches), so the
    /// frame's reference funds nothing and every owed release runs.
    fn unminted_payload_region(
        &self,
        heap: &mut crate::value::fiberheap::FiberHeap,
        payload: Value,
    ) -> Option<RuntimeRegion> {
        if self.fiber.delivery.mint_names(payload) {
            return None;
        }
        crate::value::arena::region_of(heap, payload)
    }

    /// Name this release in the free log, so `--trace=free` attributes a page to
    /// the walk rather than to whichever emitted release set the reason last.
    fn freelog_abandoned(route: &str, slot: u32, region: RuntimeRegion) {
        if crate::value::fiberheap::freelog::enabled() {
            crate::value::fiberheap::freelog::set_reason_owned(format!(
                "abandoned frame ({route} slot {slot}, runtime region {region})"
            ));
        }
    }

    /// The slot-routed half of [`Self::release_abandoned_frame`]: the releases the
    /// activation owed through `DecrefRegion`. That route's receipt is the
    /// activation map — the alloc mints the mapping and the release takes it —
    /// so a slot still mapped is a release that did not run.
    ///
    /// Restricted to the slots THIS function releases for. The map outlives a
    /// frame-replacing tail call, so it can also hold a caller's leftovers, whose
    /// references the callee's own machinery answers for; naming only this
    /// function's slots leaves those out by construction.
    fn release_abandoned_regions(&mut self, regions: &[u32], payload: Value) {
        if regions.is_empty() {
            return;
        }
        let heap = unsafe { &mut *self.heap_ptr };
        let payload_region = self.unminted_payload_region(heap, payload);
        let owed: Vec<RuntimeRegion> = {
            let Some(frame) = self.fiber.activation_region_maps.last() else {
                return;
            };
            regions
                .iter()
                .filter_map(|slot| frame.get(slot).copied())
                .filter(|m| payload_region != Some(m.region))
                .map(|m| m.region)
                .collect()
        };
        if let Some(frame) = self.fiber.activation_region_maps.last_mut() {
            for slot in regions {
                frame.remove(slot);
            }
        }
        for r in owed {
            Self::freelog_abandoned("slot route", 0, r);
            self.heap().decref_region_if_present(r);
        }
    }
}
