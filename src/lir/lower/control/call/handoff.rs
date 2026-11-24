// audited: 2026-11-23
//! What a call hands its callee: the arguments a non-tail call offers to move, and the borrowed tail arguments the runtime mints.
//!
//! docs/impl/region/handoff.md

use super::*;
use crate::hir::region::Region;
pub(super) use crate::lir::code::Offer;
use crate::lir::code::OfferBuf;
use crate::lir::lower::expr::{SlotRead, VarRead};
use crate::lir::lower::regiondecref::ReleaseRoute;
use crate::lir::lower::ValueSlot;
use rustc_hash::FxHashSet;

/// The borrowed arguments of a call: each argument the call retains, the slot
/// each is stashed in, which the call's fall-through releases, and the late
/// mints among them.
#[derive(Default)]
pub(super) struct BorrowedArgs {
    retained: FxHashSet<HirId>,
    stash: Vec<u32>,
    late_mints: OfferBuf,
}

impl BorrowedArgs {
    /// The borrowed argument `arg`, stashed in `slot`, and its late mint if it
    /// has one.
    pub(super) fn push(&mut self, arg: HirId, slot: u16, late: Option<Offer>) {
        self.retained.insert(arg);
        self.stash.push(slot.into());
        if let Some(offer) = late {
            self.late_mints.push(offer);
        }
    }

    /// The arguments the call retains, which move none of the frame's
    /// references (docs/impl/region/operand.md).
    pub(super) fn retained(&self) -> &FxHashSet<HirId> {
        &self.retained
    }

    pub(super) fn stash(&self) -> Slots<'_> {
        Slots::new(&self.stash)
    }

    pub(super) fn late_mints(&self) -> Offers<'_> {
        self.late_mints.offers()
    }
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

    /// The moves the non-tail call being lowered offers its callee.
    /// `current_hir_id` is the call's.
    ///
    /// An argument is offered where it reads a stack slot and the call node
    /// releases what the slot holds, by a plain value-routed release and by
    /// nothing else, and no other operand names the released region. The
    /// callee reads a binding or its cell. A constant callee is a native,
    /// which takes no move, so its call offers nothing.
    pub(super) fn call_move_offers(&self, func: &Hir, args: &[CallArg]) -> OfferBuf {
        let mut moves = OfferBuf::default();
        let declining_callee = match &func.kind {
            HirKind::Var(callee) => self
                .immutable_values
                .get(callee)
                .is_some_and(|v| !v.is_closure()),
            HirKind::DerefCell { cell } => !matches!(cell.kind, HirKind::Var(_)),
            _ => true,
        };
        if declining_callee {
            return moves;
        }
        let Some(released) = self
            .current_hir_id
            .and_then(|id| self.decrefs_by_decref_point.get(&id))
        else {
            return moves;
        };
        // Read only once some argument releases movably.
        let mut operands: Option<Vec<FxHashSet<Region>>> = None;
        for (position, arg) in args.iter().enumerate() {
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
            let operands = operands.get_or_insert_with(|| self.operand_regions(func, args));
            let named = |r: &Region| {
                operands
                    .iter()
                    .enumerate()
                    .any(|(j, regions)| j != position && regions.contains(r))
            };
            if moved.iter().any(named) {
                continue;
            }
            if let Some(offer) = Offer::new(position, slot) {
                moves.push(offer);
            }
        }
        moves
    }

    /// The value regions each operand of a call names: each argument's in
    /// order, then the callee's.
    fn operand_regions(&self, func: &Hir, args: &[CallArg]) -> Vec<FxHashSet<Region>> {
        args.iter()
            .map(|arg| &arg.expr)
            .chain([func])
            .map(|operand| {
                let mut regions = FxHashSet::default();
                self.region_info
                    .operand_value_regions(operand, &mut regions);
                regions
            })
            .collect()
    }

    /// The stack slot whose plain `LoadLocal` gives `h` its value: a read
    /// [`Self::var_read`] answers with a stack slot no cell wraps. `h` reads
    /// the binding, or is the `let` ANF wraps around an argument to name its
    /// value, `(let [t init] t)`.
    fn plain_local_read(&self, h: &Hir) -> Option<u16> {
        let b = match &h.kind {
            HirKind::Var(b) => b,
            HirKind::Let { bindings, body } => match (bindings.as_slice(), &body.kind) {
                ([(b, _)], HirKind::Var(read)) if read == b => b,
                _ => return None,
            },
            _ => return None,
        };
        match self.var_read(b) {
            VarRead::Slot(SlotRead::Local { slot, cell: false }) => Some(slot),
            _ => None,
        }
    }

    /// Does `r`'s release at this call load `slot` and run a plain
    /// `DecrefValueRegion`, and nothing that a move could reorder? The first is
    /// the route the call node's own release takes, and the second is the
    /// pinned set.
    fn releases_movably(&self, r: Region, slot: u16) -> bool {
        self.region_info.call_result_regions.contains(&r)
            && self.release_route(r) == ReleaseRoute::Value(ValueSlot::Local(slot))
            && !self.move_pinned.contains(&r)
    }

    /// Does the runtime mint the borrowed tail argument `arg` at the
    /// `TailCall`, rather than the frame ahead of it? Only where nothing the
    /// frame runs before the call can release the value: `arg` reads a heap
    /// constant, which the code holds, or a capture of a binding nothing
    /// reassigns, which the closure environment holds. A capture sits below
    /// `num_captures` in the environment; a captured parameter and an
    /// environment cell of this frame's own sit above it. A capture qualifies
    /// though a constant stands in for its read, as an immediate one does.
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
        let capture = self.current_self_binding != Some(*b)
            && !self.arena.get(*b).is_mutated
            && matches!(self.slot_read(b),
                Some(SlotRead::Capture { index, .. }) if index < self.num_captures);
        constant || capture
    }
}
