// audited: 2026-09-19
//! The releases an activation abandoned by an error still owed, and where
//! each tier reads the slot that names one.
//!
//! docs/impl/region/mechanism.md

use super::*;

/// Where the abandoned-frame walk reads a value route's slot
/// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
/// still owes"). Both tiers name the slot the same way — the table is the
/// function's — and differ only in where slot `s` lives.
#[derive(Clone, Copy)]
pub(crate) enum FrameLocals<'a> {
    /// An interpreter activation: slot `s` is `fiber.stack[base + s]`. The walk
    /// stamps it nil as it releases, exactly as the abandoned instruction would.
    Stack(usize),
    /// A compiled activation: slot `s` is `spill[s]`, the frame's Cranelift
    /// local spilled at the error exit. The frame returns as the walk completes,
    /// so there is no slot left to stamp and none is written back.
    ///
    /// `elle_jit_release_abandoned_frame` is its only constructor, so a build
    /// without the compiled tier never reaches this arm.
    #[cfg_attr(not(feature = "jit"), allow(dead_code))]
    Spilled(&'a [Value]),
}

impl FrameLocals<'_> {
    /// The value slot `slot` holds, or `None` where the frame has no such slot.
    fn get(&self, vm: &VM, slot: u16) -> Option<Value> {
        match self {
            FrameLocals::Stack(base) => vm.fiber.stack.get(base + slot as usize).copied(),
            FrameLocals::Spilled(spill) => spill.get(slot as usize).copied(),
        }
    }
}

impl VM {
    /// The interpreter's entry to the abandoned-frame walk: the tables come off
    /// the executing `Code`, and slot `s` lives on this activation's frame stack.
    pub(crate) fn release_abandoned_frame(&mut self, code: &crate::value::Code, payload: Value) {
        let base = self.current_frame_base();
        let slots = code.frame_release_slots().to_vec();
        let regions = code.frame_release_regions().to_vec();
        self.release_abandoned(&slots, &regions, payload, FrameLocals::Stack(base));
    }

    /// Run the releases an activation abandoned by an **error** still owed
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes").
    ///
    /// An error leaves through the signal machinery, so none of the frame's
    /// remaining instructions run — and every release it still owed is among
    /// them. `slots` names each value route by the local slot it reads: the route
    /// is `LoadLocal s; DecrefValueRegion; StoreLocal s nil`, so a slot still
    /// holding a heap value is a release that did not run, and releasing what it
    /// holds is exactly what the abandoned instruction would have done. On an
    /// interpreter frame the stamp is repeated here for the same reason the route
    /// stamps it — a slot released twice is an over-free.
    /// [`Self::release_abandoned_regions`] is the same reading of the other route,
    /// off `regions`.
    ///
    /// `payload` is the value leaving with the signal. Where the raise did NOT
    /// mint the delivery — a native hands back a value read out of an argument,
    /// and `protect` delivers it to the catcher as data — the raising frame's
    /// reference IS the delivery, so a slot naming the payload's region is
    /// skipped and its release stays owed. An emit-raised error minted the
    /// delivery itself (the ledger's `record_mint`), so nothing is exempt and the
    /// frame's reference is reclaimed here.
    ///
    /// Both tiers walk here. The compiled tier arrives through
    /// `elle_jit_release_abandoned_frame`, which reads the tables out of the stack
    /// slots its prologue materialized and hands the frame's locals over spilled.
    pub(crate) fn release_abandoned(
        &mut self,
        slots: &[u16],
        regions: &[u32],
        payload: Value,
        locals: FrameLocals<'_>,
    ) {
        self.release_abandoned_regions(regions, payload);
        if slots.is_empty() {
            return;
        }
        let heap = unsafe { &mut *self.heap_ptr };
        let payload_region = self.unminted_payload_region(heap, payload);
        for &slot in slots {
            let Some(value) = locals.get(self, slot) else {
                continue;
            };
            let heap = unsafe { &mut *self.heap_ptr };
            let Some(region) = crate::value::arena::result_region_of(heap, value) else {
                continue;
            };
            if payload_region == Some(region) {
                continue;
            }
            if let FrameLocals::Stack(base) = locals {
                self.fiber.stack[base + slot as usize] = Value::NIL;
            }
            Self::freelog_abandoned("value route", slot as u32, region);
            self.heap().decref_region(region);
        }
    }
    /// The payload region the abandoned-frame walk must leave standing, or
    /// `None` when nothing is exempt: an immediate payload has no region, and an
    /// emit-raised error minted its own delivery (the ledger's `mint_names`
    /// matches), so the frame's reference funds nothing and every owed release
    /// runs.
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
    /// Run the deferred releases of an activation the exit ABANDONS — an error
    /// no restart replays, or a squelch boundary — leaving its owner node alone
    /// (docs/impl/region/owner.md § "What an abandoned frame owes, it owes the
    /// deferred set too"). Nothing replays the frame, so this is the last chance
    /// the decref the activation took over has to run; the node's own disposal
    /// there is a separate question, since it rides out to the caller that may
    /// still park the frame.
    pub(crate) fn release_abandoned_deferred(&mut self) {
        for region in self.activation_dues().take_deferred() {
            Self::freelog_abandoned("deferred tail call", 0, region);
            self.heap().decref_region_if_present(region);
        }
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
    /// abandoned activation owed through `DecrefRegion`. That route's receipt is
    /// the activation map — the alloc mints the mapping and the release takes it —
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

#[cfg(test)]
mod tests;
