// audited: 2026-11-13
//! What a call hands its callee: the arguments a non-tail call offers to move, and the borrowed tail arguments the runtime mints.
//!
//! docs/impl/region/handoff.md

use super::*;
use crate::hir::region::Region;
pub(super) use crate::lir::code::Offer;
use rustc_hash::FxHashSet;

/// The borrowed arguments of a call: the slot each is stashed in, which the
/// call's fall-through releases, and the late mints among them, each as
/// [`Offer::word`] writes it.
#[derive(Default)]
pub(super) struct BorrowedArgs {
    pub(super) stash: Vec<u16>,
    pub(super) late_mints: Vec<u32>,
}

impl<'a> Lowerer<'a> {
    /// The regions no call may offer to move: each endpoint of an ordering
    /// edge inside its own `decref_point` bucket, and each adopt owner. Moved,
    /// such a release would run out of the order its bucket keeps, or free a
    /// subtree another release still reads. `adopt_owner` and `value_alias`
    /// are the edge maps `with_region_info` sorts the buckets with.
    ///
    /// A merge root needs no entry: its release is the merged slot's
    /// `DecrefRegion`, which [`Self::releases_movably`] already refuses.
    pub(in crate::lir::lower) fn move_pinned_regions(
        decrefs_by_decref_point: &HashMap<HirId, Vec<Region>>,
        adopt_owner: &HashMap<Region, Vec<Region>>,
        value_alias: &HashMap<Region, Vec<Region>>,
    ) -> FxHashSet<Region> {
        let mut pinned: FxHashSet<Region> = adopt_owner.values().flatten().copied().collect();
        for bucket in decrefs_by_decref_point.values() {
            let present: FxHashSet<Region> = bucket.iter().copied().collect();
            for &earlier in bucket {
                for &later in [adopt_owner, value_alias]
                    .iter()
                    .filter_map(|edges| edges.get(&earlier))
                    .flatten()
                    .filter(|later| present.contains(later))
                {
                    pinned.insert(earlier);
                    pinned.insert(later);
                }
            }
        }
        pinned
    }

    /// The moves the non-tail call being lowered offers its callee, each as
    /// [`Offer::word`] writes it. `current_hir_id` is the call's.
    ///
    /// An argument is offered where it reads a stack slot and the call node
    /// releases what the slot holds, by a plain value-routed release and by
    /// nothing else, and no other operand names the released region. The
    /// callee reads a binding or its cell. A constant callee is a native,
    /// which takes no move, so its call offers nothing.
    pub(super) fn call_move_offers(&self, func: &Hir, args: &[CallArg]) -> Vec<u32> {
        let mut words = Vec::new();
        let declining_callee = match &func.kind {
            HirKind::Var(callee) => self
                .immutable_values
                .get(callee)
                .is_some_and(|v| !v.is_closure()),
            HirKind::DerefCell { cell } => !matches!(cell.kind, HirKind::Var(_)),
            _ => true,
        };
        let Some(released) = self
            .current_hir_id
            .and_then(|id| self.decrefs_by_decref_point.get(&id))
        else {
            return words;
        };
        if declining_callee {
            return words;
        }
        for (position, arg) in args.iter().enumerate().take(64) {
            let Some(slot) = self.plain_local_read(&arg.expr) else {
                continue;
            };
            let moved: Vec<Region> = released
                .iter()
                .copied()
                .filter(|r| self.region_to_slot.get(r).and_then(|s| s.local()) == Some(slot))
                .collect();
            if moved.is_empty() || !moved.iter().all(|&r| self.releases_movably(r, slot)) {
                continue;
            }
            let mut named = FxHashSet::default();
            for (_, other) in args.iter().enumerate().filter(|(j, _)| *j != position) {
                self.region_info
                    .operand_value_regions(&other.expr, &mut named);
            }
            self.region_info.operand_value_regions(func, &mut named);
            if moved.iter().any(|r| named.contains(r)) {
                continue;
            }
            words.push(
                Offer {
                    position: position as u8,
                    slot,
                }
                .word(),
            );
        }
        words
    }

    /// The stack slot whose plain `LoadLocal` gives `h` its value, mirroring
    /// `lower_var`: a binding that is no constant, no self-reference, no cell
    /// and no upvalue. `h` reads the binding, or is the `let` ANF wraps around
    /// an argument to name its value, `(let [t init] t)`.
    fn plain_local_read(&self, h: &Hir) -> Option<u16> {
        let b = match &h.kind {
            HirKind::Var(b) => b,
            HirKind::Let { bindings, body } => match (bindings.as_slice(), &body.kind) {
                ([(b, _)], HirKind::Var(read)) if read == b => b,
                _ => return None,
            },
            _ => return None,
        };
        if self.immutable_values.contains_key(b)
            || self.current_self_binding == Some(*b)
            || self.arena.get(*b).needs_capture()
            || (self.in_lambda && self.upvalue_bindings.contains(b))
        {
            return None;
        }
        self.binding_to_slot.get(b).copied()
    }

    /// Does `r`'s release at this call load `slot` and run a plain
    /// `DecrefValueRegion`, and nothing that a move could reorder? These are
    /// the value route's refusals in `emit_decref_for_region`, each read as a
    /// refusal to move, and the pinned set.
    fn releases_movably(&self, r: Region, slot: u16) -> bool {
        let info = &self.region_info;
        info.call_result_regions.contains(&r)
            && info.merged_root(r) == r
            && !info.suppressed_decref_regions.contains(&r)
            && !info.owned_group_members.contains(&r)
            && !info.mutated_binding_value_regions.contains(&r)
            && !info.cell_release_regions.contains(&r)
            && !info.transfer_adopt_regions.contains(&r)
            && !self.reassigned_local_slots.contains(&slot)
            && !self.move_pinned.contains(&r)
    }

    /// Does the runtime mint the borrowed tail argument `arg` at the
    /// `TailCall`, rather than the frame ahead of it? Only where nothing the
    /// frame runs before the call can release the value: `arg` reads a heap
    /// constant, which the code holds, or a capture of a binding nothing
    /// reassigns, which the closure environment holds. A capture sits below
    /// `num_captures` in the environment; a captured parameter and an
    /// environment cell of this frame's own sit above it.
    pub(super) fn mints_late(&self, arg: &Hir) -> bool {
        let b = match &arg.kind {
            HirKind::Var(b) => b,
            HirKind::DerefCell { cell } => match &cell.kind {
                HirKind::Var(b) => b,
                _ => return false,
            },
            _ => return false,
        };
        if self.destructure_alias_bindings.contains(b) || self.borrowed_alias_bindings.contains(b) {
            return false;
        }
        let constant = self.immutable_values.get(b).is_some_and(|v| v.is_heap());
        let capture = self.in_lambda
            && self.upvalue_bindings.contains(b)
            && self.current_self_binding != Some(*b)
            && !self.arena.get(*b).is_mutated
            && self
                .binding_to_slot
                .get(b)
                .is_some_and(|&slot| slot < self.num_captures);
        constant || capture
    }
}
