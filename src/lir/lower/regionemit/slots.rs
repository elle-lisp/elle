// audited: 2026-09-14
//! The static slots this function's emission resolves.
//!
//! Which slot a region's value is read from, the address space that slot was
//! minted in, and the slots a builder-idiom merge collapses onto one.
//! docs/impl/region/mechanism.md
//! docs/impl/region/merging.md

use super::*;

impl<'a> Lowerer<'a> {
    /// Record the slot owning the result of an allocating
    /// expression at `hir_id`, keyed by its allocation region.
    /// Called from `lower_let` / `lower_letrec` / `lower_define`
    /// after `allocate_slot`. After ANF, every Call/Lambda/Eval/
    /// allocating-intrinsic in a consumer position is bound to a
    /// synthetic Let — so this map covers the result via its binding
    /// slot directly, without a separate stash-and-reload slot.
    ///
    /// Takes the BINDING whose slot this is, never a ready-made
    /// [`ValueSlot`]: the address space comes from `value_slot_for`, so a binder
    /// form cannot record a space its own allocation did not use.
    pub(in crate::lir::lower) fn record_region_slot(
        &mut self,
        hir_id: HirId,
        binding: Binding,
        slot: u16,
    ) {
        if let Some(&r) = self.region_info.alloc_region.get(&hir_id) {
            let space = self.value_slot_for(binding, slot);
            self.region_to_slot.insert(r, space);
        }
    }

    /// The address space `allocate_slot_routed` minted this binding's slot from:
    /// an in-lambda captured binding lives in the env, everything else on the
    /// stack — including an in-lambda binding whose forward cell is COMPILED,
    /// whose slot holds the `MakeCaptureCell` itself. The one place that
    /// decision is re-derived, so a recording site cannot disagree with the
    /// allocation.
    pub(in crate::lir::lower) fn value_slot_for(
        &self,
        binding: Binding,
        slot: u16,
    ) -> super::ValueSlot {
        if self.in_lambda
            && self.arena.get(binding).needs_capture()
            && !self.compiled_cell_bindings.contains(&binding)
        {
            super::ValueSlot::Env(slot)
        } else {
            super::ValueSlot::Local(slot)
        }
    }

    /// Record `region_to_slot[cell_r] = slot` for a captured local's env-cell
    /// placeholder (the analysis put it in `binding_source_regions[binding]` and
    /// `cell_release_regions`; see `RegionInference::env_cell_placeholder`). This
    /// lets `emit_decrefs_for` release the env cell at the binding's last use via
    /// `LoadCaptureRaw` + `DecrefCellRegion`. `slot` is the binding's env/upvalue
    /// index (the same index `StoreCapture`/`LoadCapture` use). Mirrors the
    /// captured-param recording in `lower_lambda_body`, but per-binding because a
    /// local's slot is only known once its define/let is lowered (mid-body), and
    /// `record_region_slot` keys off `alloc_region`, which a phantom placeholder
    /// is deliberately absent from.
    pub(in crate::lir::lower) fn record_env_cell_release_slot(
        &mut self,
        binding: Binding,
        slot: u16,
    ) {
        let Some(regions) = self.region_info.binding_source_regions.get(&binding) else {
            return;
        };
        for &r in regions.clone().iter() {
            if self.region_info.cell_release_regions.contains(&r) {
                self.region_to_slot.insert(r, super::ValueSlot::Env(slot));
            }
        }
    }

    /// Record the static slots a builder-idiom merge collapses two or more
    /// allocations onto — the `merged_slots` set the runtime mint-or-reuses
    /// (docs/impl/region/merging.md § Merging) — into the current function, after its
    /// body is lowered (so `region_to_table` holds this function's slots). Called at
    /// each function's finalization (the entry in `lower`, every lambda in
    /// `lower_lambda_body`).
    ///
    /// Every member of a merge tree resolves to the **root's** slot — `static_slot`
    /// canonicalizes through `merged_root` — so the shared slot is the root's, read
    /// from the (root-keyed) `region_to_table`. The runtime-population guard
    /// (`emitted_alloc_regions`, mirroring `coalescible_region`) keeps a slot no
    /// allocation in THIS function stamped out of the set: such a slot has no
    /// activation mapping to reuse. With no merge (`merged_parent` empty — a
    /// compile whose `%pair` allocation nodes seed no builder idiom) this returns
    /// immediately and `merged_slots` stays empty, so mint-or-reuse is the plain
    /// mint on that path.
    pub(in crate::lir::lower) fn record_merged_slots(&mut self) {
        if self.region_info.merged_parent.is_empty() {
            return;
        }
        let mut merged: rustc_hash::FxHashSet<StaticRegion> = rustc_hash::FxHashSet::default();
        for &child in self.region_info.merged_parent.keys() {
            // The merged slot is the root's; read it (never mint) and keep it only
            // if an allocation emitted in this function stamped it.
            let root = self.region_info.merged_root(child);
            if let Some(&slot) = self.region_to_table.get(&root) {
                if self.emitted_alloc_regions.contains(&slot) {
                    merged.insert(slot);
                }
            }
        }
        // Decref-dominance: each merged slot must carry exactly one `DecrefRegion`
        // (the root's; non-root children are suppressed in `emit_decrefs_for`), so
        // the single drop frees the whole merged region and clears the slot once
        // per activation — the invariant mint-or-reuse relies on (the next loop
        // iteration's child re-mints against the cleared slot). A merge that cannot
        // prove this is never recorded; the unmerged baseline (always legal)
        // stands.
        #[cfg(debug_assertions)]
        for &slot in &merged {
            let decref_count = self
                .current_func
                .blocks
                .iter()
                .flat_map(|b| b.instructions.iter())
                .filter(|si| {
                    matches!(&si.instr, LirInstr::DecrefRegion { region_id } if *region_id == slot)
                })
                .count();
            debug_assert_eq!(
                decref_count, 1,
                "merged slot {} must carry exactly one DecrefRegion \
                 (decref-dominance for mint-or-reuse); got {}",
                slot, decref_count
            );
        }
        self.current_func.merged_slots = merged.into_iter().collect();
    }
}
