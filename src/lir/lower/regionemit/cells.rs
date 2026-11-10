// audited: 2026-11-10
//! The retains a 1-slot container's binder takes for the init it stores.
//!
//! docs/impl/region/bindings.md
//! docs/impl/region/reads.md

use super::*;

impl<'a> Lowerer<'a> {
    /// The reader half of the 1-slot container: when a binding init at `hir_id`
    /// is a whole-value read of an `is_one_slot_container` binding
    /// (`RegionInfo::counted_cell_read_sites`), the reader takes its OWN counted
    /// reference — Rule 5's "new reference" pass-through — so the container's
    /// next overwrite cannot free the value under the reader. Both realizations
    /// re-store the same way as far as the reader is concerned: a capture cell
    /// through `capture_store_with_rebind`, an uncelled `@`-mutable local through
    /// the compiler's own drop-on-overwrite (docs/impl/region/bindings.md). The
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
    /// Pinned by tests/impl/region-reassign-captured-cell-reader.lisp (the
    /// fn-local binder) and tests/impl/region-container-read-toplevel-uaf.lisp
    /// (the file-letrec binder).
    pub(in crate::lir::lower) fn emit_counted_cell_read_retain(&mut self, hir_id: HirId, src: Reg) {
        if self.region_info.counted_cell_read_sites.contains(&hir_id) {
            self.emit(InstrRef::IncrefValueRegion { src });
        }
    }

    /// The writer half of a fn-local 1-slot container whose INIT value carries a
    /// second name (`RegionInfo::counted_cell_init_sites`): the cell takes that
    /// value by a COUNTED store rather than by donation, so the alias keeps the
    /// producer's reference and the ordinary decref that releases it
    /// (docs/impl/region/bindings.md). The retain is balanced by the same
    /// drop-on-overwrite that balances every later store — the cell's own
    /// reference, dropped at the first overwrite or at the content drop.
    ///
    /// Emitted while the init value is on the operand-stack top, right after
    /// `lower_expr(init)` and before the slot store: `IncrefValueRegion` peeks the
    /// top and does not pop, so the value stays in place for the store. No-op
    /// unless `hir_id` is a counted-init site. Pinned by
    /// tests/impl/region-cell-aliased-init.lisp.
    pub(in crate::lir::lower) fn emit_counted_cell_init_retain(&mut self, hir_id: HirId, src: Reg) {
        if self.region_info.counted_cell_init_sites.contains(&hir_id) {
            self.emit(InstrRef::IncrefValueRegion { src });
        }
    }
}
