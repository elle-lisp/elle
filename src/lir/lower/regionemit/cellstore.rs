// audited: 2026-11-10
//! The store of a captured binding's init into its cell, and the drop a reassigned binding's binder owes that init.
//!
//! docs/impl/region/cells.md
//! docs/impl/region/mechanism.md
//! docs/impl/region/adopt.md

use super::*;

/// How a reassigned captured binding's binder releases its init's producer
/// reference after the cell store.
#[derive(Clone, Copy)]
enum CapturedInitDrop {
    /// A fresh local allocation: release its region by the slot that names it.
    Slot(crate::hir::region::StaticRegion),
    /// Any other allocation, a call result among them: release by value.
    Value,
}

impl<'a> Lowerer<'a> {
    /// Store a captured binding's init value into the nil-valued
    /// `MakeCaptureCell` already sitting in its `slot` — put there by the
    /// `lower_begin`/`lower_letrec` pre-pass, or by `lower_let` immediately
    /// ahead of this call. Every binder that mints a compiled cell reaches the
    /// store through here, so the cell's membership reference and the init drop
    /// below cannot come apart at one of them (docs/impl/region/cells.md).
    ///
    /// `reassigned` selects how the init value's ALLOC reference is dropped:
    ///
    /// - `false` (the binding is never reassigned): the ordinary route. The
    ///   caller leaves `record_region_slot(init → slot)` in place, so the init
    ///   region's `DecrefValueRegion` reloads the cell at its `decref_point` and
    ///   `result_region_of` unwraps to the cell's content — which, with no
    ///   reassignment, is always exactly this init value. Nothing extra here.
    ///
    /// - `true` (the binding is reassigned): the cell content CHANGES, so a
    ///   later slot-load + unwrap would free a different, live value
    ///   (tests/impl/region-capture-cell-reassign-loop-uaf.lisp). The caller
    ///   SKIPS `record_region_slot` for the init, and we drop its alloc
    ///   reference HERE off `value_reg` directly, where the init allocated one
    ///   ([`Self::captured_init_drop`]). `StoreCaptureCell`
    ///   (`handle_update_capture`) already raised the value's region for the
    ///   cell's membership; this releases the producer's reference, leaving
    ///   exactly the cell's. That membership reference is reclaimed by the cell's
    ///   free cascade (the final value) or by the next reassignment's
    ///   drop-on-overwrite.
    ///
    ///   This drop is the **decref side** of compile-time region selection
    ///   (docs/impl/region/mechanism.md): when `value` is a fresh
    ///   local allocation whose region is a known slot (the usual case for a
    ///   captured binding's init), the release is slot-resolved
    ///   (`DecrefRegion`, guarded under `debug_assertions` by the equivalence
    ///   oracle `AssertRegionMatches`), otherwise it stays value-resolved. The
    ///   guard refuses any captured/cross-thread region (`coalescible_region` —
    ///   the slot must be stamped by an allocation emitted in this function).
    ///   `DecrefRegion` touches no operand stack, so `value_reg` is left on top
    ///   exactly as the never-reassigned (`false`) path leaves it — a benign
    ///   orphan the block-end cleanup consumes.
    pub(in crate::lir::lower) fn store_captured_cell_init(
        &mut self,
        binding: Binding,
        slot: u16,
        value_reg: Reg,
        value: &Hir,
        reassigned: bool,
    ) {
        let cell_reg = self.fresh_reg();
        self.emit(InstrRef::LoadLocal {
            dst: cell_reg,
            slot,
        });
        self.emit(InstrRef::StoreCaptureCell {
            cell: cell_reg,
            value: value_reg,
        });
        // `cell ⊇ content`: adopt the just-stored content into the cell's OWN region when
        // the ownership forest admitted this cell's `closure ⊇ cell ⊇ content` clique. The
        // `StoreCaptureCell` above already increfed the content (`handle_update_capture`
        // → `capture_store_with_rebind`), so the adopt consumes that count (the
        // funnel-adopt discipline). No-op for a re-storable cell (never in
        // `cell_content_adopt_bindings`), so it is emitted here BEFORE the reassigned
        // drop below without disturbing it.
        self.maybe_emit_cell_content_adopt(binding, cell_reg, value_reg);
        match self.captured_init_drop(value, reassigned) {
            Some(CapturedInitDrop::Slot(region_id)) => {
                #[cfg(debug_assertions)]
                self.emit(InstrRef::AssertRegionMatches {
                    region_id,
                    src: value_reg,
                });
                self.emit(InstrRef::DecrefRegion { region_id });
            }
            Some(CapturedInitDrop::Value) => {
                self.emit(InstrRef::DecrefValueRegion { src: value_reg })
            }
            None => {}
        }
    }

    /// Store a captured binding's init into its `populate_env` env cell at
    /// `index`, and drop the init's producer reference when the binding is
    /// `reassigned`, as [`Self::store_captured_cell_init`] does for a compiled
    /// cell. Every binder of an env-celled binding stores its init through here.
    ///
    /// `StoreCapture` consumes `value_reg`, so the oracle runs ahead of the store
    /// while the value is on top, and a value-resolved drop reloads the cell RAW:
    /// `result_region_of` unwraps it to the content, which is still the init.
    pub(in crate::lir::lower) fn store_env_cell_init(
        &mut self,
        index: u16,
        value_reg: Reg,
        value: &Hir,
        reassigned: bool,
    ) {
        let drop = self.captured_init_drop(value, reassigned);
        #[cfg(debug_assertions)]
        if let Some(CapturedInitDrop::Slot(region_id)) = drop {
            self.emit(InstrRef::AssertRegionMatches {
                region_id,
                src: value_reg,
            });
        }
        self.emit(InstrRef::StoreCapture {
            index,
            src: value_reg,
        });
        match drop {
            Some(CapturedInitDrop::Slot(region_id)) => {
                self.emit(InstrRef::DecrefRegion { region_id })
            }
            Some(CapturedInitDrop::Value) => {
                let cell_reg = self.fresh_reg();
                self.emit(InstrRef::LoadCaptureRaw {
                    dst: cell_reg,
                    index,
                });
                self.emit(InstrRef::DecrefValueRegion { src: cell_reg });
            }
            None => {}
        }
    }

    /// The drop a captured binding's binder owes its init after the cell store
    /// (docs/impl/region/cells.md). `None` unless the binding is `reassigned`:
    /// a stable cell's init keeps the routed release through its slot. `None`
    /// too for an init that allocates nothing, such as a `Var` naming another
    /// binding: it borrows that binding's reference and holds none of its own.
    fn captured_init_drop(&self, value: &Hir, reassigned: bool) -> Option<CapturedInitDrop> {
        if !reassigned || !self.region_info.alloc_region.contains_key(&value.id) {
            return None;
        }
        let coalesced = self.coalescible_region(value);
        super::rcstats::record_captured_init(coalesced.is_some());
        Some(coalesced.map_or(CapturedInitDrop::Value, CapturedInitDrop::Slot))
    }
    /// Emit the `cell ⊇ content` adopt for `binding` if the ownership forest admitted it
    /// (`RegionInfo::cell_content_adopt_bindings`): `AdoptCellRegion(cell, content)` links
    /// the content's runtime region into the CELL's own region (`region_of`, never the
    /// unwrapped content — that is exactly what `AdoptRegion` would do wrongly), so a
    /// local `closure ⊇ cell ⊇ content` clique frees as one subtree. Both operands are in
    /// registers at the cell store (`cell_reg` the just-loaded cell, `value_reg` the
    /// content), so no slot reload is needed. No-op for a binding whose cell was not
    /// admitted (Shared baseline) — empty set without the forest.
    pub(in crate::lir::lower) fn maybe_emit_cell_content_adopt(
        &mut self,
        binding: Binding,
        cell_reg: Reg,
        value_reg: Reg,
    ) {
        if self
            .region_info
            .cell_content_adopt_bindings
            .contains(&binding)
        {
            self.emit(InstrRef::AdoptCellRegion {
                parent: cell_reg,
                child: value_reg,
            });
            if crate::config::get().has_trace("rc") {
                eprintln!(
                    "[trace:rc:emit] adopt_cell_region cell⊇content binding={:?}",
                    binding
                );
            }
        }
    }
}
