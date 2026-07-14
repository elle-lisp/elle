//! The ownership forest: adoption, ownership queries, and transfer.
//!
//! An `Owned` region is reclaimed only by its owner's subtree drop, never by a
//! count of its own (docs/impl/region/ownership.md § "Adoption and subtree
//! drop"). These primitives move regions between `Counted` and `Owned` and
//! re-home whole subtrees while keeping the forest's forward/back edges
//! consistent — the structural guarantee that makes "owned-and-RC'd"
//! unrepresentable.

use super::*;

impl RegionStore {
    /// Link `child` as an Owned member of `parent`'s subtree — the runtime
    /// `AdoptRegion` (docs/impl/region/ownership.md § "Adoption and subtree drop").
    /// **Moves** `child` from `Counted` into `Owned`, *consuming* its reference
    /// count: from here the child is reclaimed only by `parent`'s subtree drop
    /// (`free_runtime_region_pages`), never by its own RC reaching zero — there is
    /// no count left to reach zero. No incref — an interior ownership edge is not
    /// reference-counted (the subtree frees as a unit).
    ///
    /// A region is adopted **at most once**: a second adoption would mean two
    /// owners, so it finds the child already `Owned` and is a debug-asserted bug
    /// (the inference adopts each member once). This is the structural guard that
    /// "owned-and-RC'd" cannot arise — the count is gone after the first adoption,
    /// not merely frozen-and-ignored.
    ///
    /// Both regions are `ensure`d so the edge survives even if neither has
    /// allocated yet (a conditional alloc that never executed leaves an empty but
    /// present `Counted(1)` entry, exactly as the baseline RC path tolerates).
    pub(crate) fn adopt_region(&mut self, parent: RuntimeRegion, child: RuntimeRegion) {
        self.ensure(parent);
        self.ensure(child);
        let c = self.regions[child.get() as usize].as_mut().unwrap();
        debug_assert!(
            matches!(c.reclaim, Reclaim::Counted(_)),
            "region {child} adopted while already Owned — a region has at most one \
             owner; owned-and-RC'd is unrepresentable, so a double adoption is a bug \
             (docs/impl/region/ownership.md § 'The runtime: a reclamation typestate')",
        );
        c.reclaim = Reclaim::Owned { owner: parent };
        self.regions[parent.get() as usize]
            .as_mut()
            .unwrap()
            .owned_children
            .push(child);
    }

    /// Whether `id` is currently an **Owned** forest member (adopted — reclaimed
    /// only by its owner's subtree drop). False for an absent or `Counted`
    /// region. The `AdoptIntoActivation` handlers read this to make the
    /// consumer-facing adopt channel **idempotent**: a region delivered to the
    /// channel a second time (a masked-`:error` fiber restarted after handing
    /// out the same payload) is left with its first owner instead of tripping
    /// the one-owner assert in [`Self::adopt_region`].
    pub(crate) fn region_is_owned(&self, id: RuntimeRegion) -> bool {
        self.regions
            .get(id.get() as usize)
            .and_then(|s| s.as_ref())
            .is_some_and(|e| matches!(e.reclaim, Reclaim::Owned { .. }))
    }

    /// Extract `child` from its owner's subtree — the moves-out counterpart of
    /// [`Self::adopt_region`] (docs/impl/region/ownership.md § "Adoption and subtree
    /// drop"). When a `moves_out` funnel (`%pop`) removes an element that was
    /// adopted into its container's Owned subtree, the element LEAVES the container
    /// and becomes the call's result — so it must no longer be reclaimed by the
    /// container's subtree drop. **Moves** `child` from `Owned` back to `Counted`
    /// and unlinks it from its owner's `owned_children`. The forward/back edges
    /// stay consistent (the child recorded
    /// its owner; we remove it from exactly that owner's set). A `Counted` child is
    /// left untouched (an idempotent no-op): a non-adopted element takes the
    /// ordinary RC moves-out path (escape-retain + un-record + decref), not this.
    ///
    /// The rebuilt count is **1 + the recorded external incoming edges**: the 1 is
    /// the caller's moves-out reference (the result, whose `DecrefValueRegion`
    /// reclaims it — on an Owned region `incref`/`decref` are inert, so the
    /// escape-retain the Counted path uses cannot establish that reference here;
    /// this move IS the retain). Each admitted incoming edge is a live container
    /// still holding a value in the child's region — the pop funnel un-records the
    /// popped container edge *before* extracting, so what remains is genuinely
    /// external and releases through the ordinary unrecord+decref / frontier
    /// cascade of its holder. The child's own subtree's back-edges are excluded,
    /// exactly as the drop-time rescue excludes them (they release only at the
    /// child's own drop; docs/impl/region/ownership.md § "The incoming edge table
    /// and the external-reference rescue").
    pub(crate) fn extract_owned_region(&mut self, child: RuntimeRegion) {
        let owner = match self
            .regions
            .get(child.get() as usize)
            .and_then(|s| s.as_ref())
        {
            Some(e) => match e.reclaim {
                Reclaim::Owned { owner } => owner,
                // Counted — the ordinary RC moves-out path owns it.
                Reclaim::Counted(_) => return,
            },
            None => return,
        };
        // The child's own surviving subtree, whose back-edges are not counted.
        let mut subtree: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut stack = vec![child];
        while let Some(m) = stack.pop() {
            if !subtree.insert(m.get()) {
                continue;
            }
            if let Some(e) = self.regions[m.get() as usize].as_ref() {
                stack.extend(e.owned_children.iter().copied());
            }
        }
        let entry = self.regions[child.get() as usize].as_mut().unwrap();
        let external: u32 = entry
            .incoming
            .iter()
            .filter(|(s, _)| !subtree.contains(&s.get()))
            .map(|(_, &n)| n)
            .sum();
        entry.reclaim = Reclaim::Counted(1 + external);
        if let Some(owner_entry) = self
            .regions
            .get_mut(owner.get() as usize)
            .and_then(|s| s.as_mut())
        {
            owner_entry.owned_children.retain(|&c| c != child);
        }
    }

    /// Hand `from`'s whole direct `owned_children` set to `to` — the ownership-
    /// **transfer** primitive of the forest (docs/impl/region/ownership.md § "The
    /// runtime: a reclamation typestate"). Each child is re-stamped
    /// `Owned { owner: to }` and the set is appended to `to`'s children: a move,
    /// never a copy, so the forest's forward/back edges stay consistent (the
    /// subtree-drop walk debug-asserts them) and no child gains a second owner.
    /// Neither endpoint's own reclaim mode changes and no count is created or
    /// consumed — the children were `Owned` and stay `Owned`; only the owner whose
    /// demise reclaims them changes. A self-reparent, an absent `from`, or an
    /// empty child set is a no-op (`to` is not even `ensure`d, so a transfer of
    /// nothing mints nothing).
    pub(crate) fn reparent_owned_children(&mut self, from: RuntimeRegion, to: RuntimeRegion) {
        if from == to {
            return;
        }
        let children = match self
            .regions
            .get_mut(from.get() as usize)
            .and_then(|s| s.as_mut())
        {
            Some(entry) => std::mem::take(&mut entry.owned_children),
            None => return,
        };
        if children.is_empty() {
            return;
        }
        self.ensure(to);
        for &child in &children {
            let entry = self.regions[child.get() as usize]
                .as_mut()
                .expect("an owned child has a live entry (freed only by its owner's drop)");
            debug_assert!(
                matches!(entry.reclaim, Reclaim::Owned { owner } if owner == from),
                "reparent_owned_children({from} -> {to}): child {child} does not \
                 record {from} as its owner — forward/back edge inconsistency \
                 (docs/impl/region/ownership.md § 'The runtime: a reclamation typestate')",
            );
            entry.reclaim = Reclaim::Owned { owner: to };
        }
        self.regions[to.get() as usize]
            .as_mut()
            .unwrap()
            .owned_children
            .extend(children);
    }
}
