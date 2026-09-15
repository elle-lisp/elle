// audited: 2026-09-14
//! What a 1-slot container's binder emits: the retains a read and an aliased
//! init take, and the cell store that ends a reassigned binding's init claim.
//! docs/impl/region/bindings.md
//! docs/impl/region/cells.md

use super::*;

impl<'a> Lowerer<'a> {
    /// The reader half of the 1-slot container: when a binding init at `hir_id`
    /// is a whole-value read of an `is_one_slot_container` binding
    /// (`RegionInfo::counted_cell_read_sites`), the reader takes its OWN counted
    /// reference — Rule 5's "new reference" pass-through — so the container's
    /// next overwrite cannot free the value under the reader. Both realizations
    /// re-store the same way as far as the reader is concerned: a capture cell
    /// through `capture_store_with_rebind`, an uncelled `@`-mutable local through
    /// the compiler's own drop-on-overwrite (docs/impl/region/bindings.md § "A
    /// whole-value read of a 1-slot container takes a counted reference"). The
    /// balancing `DecrefValueRegion` fires at the reader's last use: the walk
    /// minted the read's placeholder region at `hir_id`, so it lands in
    /// `call_result_regions` and its `decref_point` is the reader's last use.
    ///
    /// Emitted while the read value is on the operand-stack top (right after
    /// `lower_expr(init)`, before the slot store) — `IncrefValueRegion` peeks the
    /// top and does not pop, so the value stays in place for the store. Both
    /// binder arms that record a read call this, `lower_let` and `lower_letrec`:
    /// the container's donation is granted on the strength of the reader's own
    /// reference, so a binder that recorded the read and retained nothing frees
    /// the value under its reader. No-op unless `hir_id` is a counted read site.
    /// Pinned by tests/elle/region-reassign-captured-cell-reader.lisp (the
    /// fn-local binder) and `region_container_read_toplevel_uaf` (the
    /// file-letrec binder).
    pub(in crate::lir::lower) fn emit_counted_cell_read_retain(&mut self, hir_id: HirId, src: Reg) {
        if self.region_info.counted_cell_read_sites.contains(&hir_id) {
            self.emit(LirInstr::IncrefValueRegion { src });
        }
    }

    /// The writer half of a fn-local 1-slot container whose INIT value carries a
    /// second name (`RegionInfo::counted_cell_init_sites`): the cell takes that
    /// value by a COUNTED store rather than by donation, so the alias keeps the
    /// producer's reference and the ordinary decref that releases it
    /// (docs/impl/region/bindings.md § "What the cell donates it must hold alone;
    /// what it counts it need not"). The retain is balanced by the same
    /// drop-on-overwrite that balances every later store — the cell's own
    /// reference, dropped at the first overwrite or at the content drop.
    ///
    /// Emitted while the init value is on the operand-stack top, right after
    /// `lower_expr(init)` and before the slot store: `IncrefValueRegion` peeks the
    /// top and does not pop, so the value stays in place for the store. No-op
    /// unless `hir_id` is a counted-init site. Pinned by
    /// tests/elle/region-cell-aliased-init.lisp.
    pub(in crate::lir::lower) fn emit_counted_cell_init_retain(&mut self, hir_id: HirId, src: Reg) {
        if self.region_info.counted_cell_init_sites.contains(&hir_id) {
            self.emit(LirInstr::IncrefValueRegion { src });
        }
    }
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
    ///   later slot-load + unwrap would free a different, live value (the
    ///   capture-cell reassign UAF; region-capture-cell-reassign-uaf.lisp). The
    ///   caller SKIPS `record_region_slot` for the init, and we drop its alloc
    ///   reference HERE off `value_reg` directly. `StoreCaptureCell`
    ///   (`handle_update_capture`) already raised the value's region for the
    ///   cell's membership; this releases the producer's reference, leaving
    ///   exactly the cell's. That membership reference is reclaimed by the cell's
    ///   free cascade (the final value) or by the next reassignment's
    ///   drop-on-overwrite.
    ///
    ///   This drop is transform 1's **decref side** (docs/impl/region/mechanism.md
    ///   § "Compile-time region selection (coalescing)"): when `value` is a fresh
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
        self.emit(LirInstr::LoadLocal {
            dst: cell_reg,
            slot,
        });
        self.emit(LirInstr::StoreCaptureCell {
            cell: cell_reg,
            value: value_reg,
        });
        // `cell ⊇ content`: adopt the just-stored content into the cell's OWN region when
        // the ownership forest admitted this cell's `closure ⊇ cell ⊇ content` clique. The
        // `StoreCaptureCell` above already increfed the content via the alloc-scan, so the
        // adopt consumes that count (the funnel-adopt discipline). No-op for a
        // re-storable cell (never in `cell_content_adopt_bindings`), so it is emitted here
        // BEFORE the reassigned drop below without disturbing it.
        self.maybe_emit_cell_content_adopt(binding, cell_reg, value_reg);
        if reassigned {
            let coalesced = self.coalescible_region(value);
            super::rcstats::record_captured_init(coalesced.is_some());
            match coalesced {
                Some(region_id) => {
                    #[cfg(debug_assertions)]
                    self.emit(LirInstr::AssertRegionMatches {
                        region_id,
                        src: value_reg,
                    });
                    self.emit(LirInstr::DecrefRegion { region_id });
                }
                None => self.emit(LirInstr::DecrefValueRegion { src: value_reg }),
            }
        }
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
            self.emit(LirInstr::AdoptCellRegion {
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
