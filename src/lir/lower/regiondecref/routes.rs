// audited: 2026-09-23
//! The release routes a region's demise can take: a value off its slot, an env cell's box, a 1-slot container's content, a co-owned group.
//!
//! docs/impl/region/replicate.md
//! docs/impl/region/compensate.md
//! docs/impl/region/bindings.md

use super::*;

impl<'a> Lowerer<'a> {
    /// Load what `slot` holds, release that value's RUNTIME region, and stamp the
    /// slot `nil`.
    ///
    /// The one release shape that may be REPLICATED: whichever copy a path reaches
    /// first does the work, and any later copy loads `nil`, whose release is a
    /// no-op (`self_cancelling_run`). The stamp serves a second reader too — a
    /// slot a later arm or a later iteration reuses must not be mistaken for the
    /// value this release freed.
    pub(super) fn emit_slot_value_release(&mut self, slot: u16) {
        let val_reg = self.fresh_reg();
        self.emit(LirInstr::LoadLocal { dst: val_reg, slot });
        self.emit(LirInstr::DecrefValueRegion { src: val_reg });
        if let Ok(nil_reg) = self.emit_const(crate::lir::LirConst::Nil) {
            self.emit(LirInstr::StoreLocal { slot, src: nil_reg });
        }
    }

    /// Release a captured env cell (an `@x` lbox / captured-local cell) at
    /// `index`, the upvalue/env slot the binding's cell lives in.
    ///
    /// Load the CELL itself (raw, no deref) and free the CELL's OWN region via
    /// `DecrefCellRegion` (`region_of`) — never unwrap to the inner value's
    /// caller-owned region. The capturing closure's counted `closure ⊇ cell` edge
    /// keeps the box alive past this release until that closure's region cascade
    /// frees it.
    ///
    /// The run leaves the env slot exactly as it was, which is why it is not
    /// self-cancelling and cannot be replicated across a branch merge
    /// (`self_cancelling_run`). Its three emission sites are therefore mutually
    /// exclusive by arm structure rather than by a nil-stamp: the region's own
    /// `decref_point`, a dead sibling arm's head compensation, and a reading
    /// sibling arm's tail compensation (docs/impl/region/compensate.md).
    pub(super) fn emit_cell_region_release(&mut self, index: u16, site: HirId) {
        let val_reg = self.fresh_reg();
        self.emit(LirInstr::LoadCaptureRaw {
            dst: val_reg,
            index,
        });
        self.emit(LirInstr::DecrefCellRegion { src: val_reg });
        if crate::config::get().has_trace("rc") {
            eprintln!(
                "[trace:rc:emit] emit_decref_cell_region hir_id={:?} upvalue_slot={} span={}",
                site, index, self.current_span
            );
        }
    }

    /// Drop the current content of every fn-local 1-slot container whose scope
    /// demise is this node (docs/impl/region/bindings.md).
    ///
    /// The cell holds ONE counted reference to whatever it points at. For every
    /// value the cell displaces, that reference dies at the overwrite
    /// (`lower_define`'s drop-on-overwrite, where the slot still names it); for
    /// the final, never-overwritten content there is no overwrite, so the
    /// reference dies here, where the binding's scope does. The value route is
    /// the only correct one: which value the cell holds at scope exit is a
    /// runtime fact, and loading the slot reads exactly that (`nil` when the
    /// cell was never written, whose release is a no-op).
    ///
    /// This is the one place the reassigned binding's slot IS a release route —
    /// precisely because the release names the slot's CURRENT occupant rather
    /// than some earlier value whose region the compiler picked (the mis-target
    /// `emit_decref_for_region` refuses). The nil-stamp keeps a later reuse of
    /// the slot from being mistaken for the freed value.
    pub(super) fn emit_cell_content_drops(&mut self, hir_id: HirId) {
        let bindings = match self.cell_drops_by_demise.get(&hir_id) {
            Some(bs) => bs.clone(),
            None => return,
        };
        for b in bindings {
            // An env-celled binding is absent by construction (the walk excludes
            // `needs_capture` from both container maps — the capture cell's
            // update opcode owns its RC), so a missing slot means this binding
            // was never lowered in this function; skip rather than guess.
            let Some(&slot) = self.binding_to_slot.get(&b) else {
                continue;
            };
            self.emit_slot_value_release(slot);
            if crate::config::get().has_trace("rc") {
                eprintln!(
                    "[trace:rc:emit] cell_content_drop binding={:?} local_slot={} demise={:?} span={}",
                    b, slot, hir_id, self.current_span
                );
            }
        }
    }

    /// Emit `FreeRegionGroup` for a co-owned region group: load every member's
    /// value from its binding slot to drive the value-resolved free, then emit the
    /// one instruction that frees the whole set as a unit. A member with no
    /// value-resolved home (no `region_to_slot` entry) leaves the group unfreed —
    /// the always-legal fallback, where the members stay independently RC'd —
    /// mirroring `emit_adopt_region`'s missing-slot skip.
    pub(super) fn emit_free_region_group(&mut self, members: &[crate::hir::region::Region]) {
        let mut regs = Vec::with_capacity(members.len());
        for &m in members {
            // Stack-only emission: an env-celled member leaves the group
            // unfreed, the same always-legal fallback a missing slot takes.
            let Some(slot) = self.region_to_slot.get(&m).and_then(|s| s.local()) else {
                return;
            };
            let reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: reg, slot });
            regs.push(reg);
        }
        if regs.is_empty() {
            return;
        }
        if crate::config::get().has_trace("rc") {
            eprintln!(
                "[trace:rc:emit] free_region_group members={:?}",
                members.iter().map(|r| r.0).collect::<Vec<_>>()
            );
        }
        self.emit(LirInstr::FreeRegionGroup { members: regs });
    }
}
