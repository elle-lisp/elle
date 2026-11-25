// audited: 2026-11-25
//! The wholesale region free: tear down an owned subtree or a co-owned group, and cascade its Shared frontier.
//!
//! docs/impl/region/ownership.md
//! docs/impl/region/owner.md

use super::*;

impl RegionStore {
    /// Free the entire **owned subtree** rooted at `id` — `id` plus every region
    /// transitively reachable through `owned_children` — as a unit (subtree drop,
    /// docs/impl/region/ownership.md). For a region with no
    /// owned children this is an ordinary single-region free; for an owner it frees
    /// the whole subtree, interior reference cycles included.
    ///
    /// **Phased, so the debug oracle never reads a freed sibling.** The production
    /// frontier comes from each member's recorded `outgoing` edge table
    /// (docs/impl/region/ownership.md), so reclamation derefs **no** heap page. In
    /// debug builds an equivalence oracle also scans member contents
    /// (`find_region_cross_refs` → `owning_region`, which derefs each pointer's
    /// page) and asserts it matches the table. That scan must run while all member
    /// pages are still mapped, which the phase split guarantees:
    ///
    /// 0. **Collect and rescue** — walk the subtree read-only, then convert to
    ///    `Counted` any member still externally referenced per the recorded edge
    ///    tables, pruning it (with its own subtree) from the dying set
    ///    (docs/impl/region/ownership.md). Walking `owned_children` is Rust-side, so
    ///    collecting the subtree never derefs a heap page.
    /// 1. **Unindex** every still-dying member (take its entry out of `regions`, so
    ///    `owning_region` resolves its pages to no region) while its pages stay mapped — the
    ///    moved-out entry still owns them.
    /// 2. **Frontier** — walk every member's recorded `outgoing` table. A target still
    ///    indexed is a genuinely-**Shared** frontier ref to cascade. A target unindexed
    ///    in phase 1 is interior to the freed set and dropped: the cycle reclaims with
    ///    the set and never cascades. The debug oracle's content scan runs here too, while
    ///    all member pages are mapped, so an interior reference cycle (the
    ///    `(push a b)(push b a)` knot) never reads a sibling's returned page.
    /// 3. **Tear down** every member's pages and bump its generation (a later stale
    ///    deref detonates at the next debug `region_of`), recycling the id and
    ///    keeping its entry for the next region.
    ///
    /// Steps 0–3 are [`Self::teardown_set`], which leaves the Shared frontier for its
    /// caller rather than freeing it. [`Self::free_region_set`] then **cascades** those
    /// refs iteratively: a frontier ref that reaches rc 0 becomes another set fed back
    /// through the same steps. So a deep chain of cross-region references frees in
    /// O(1) native stack. Their pages lie outside the subtree, untouched by the
    /// teardown.
    ///
    /// Production reads no page to discover an edge; the only page reads are the debug
    /// oracle's scan (phase 2) and teardown (phase 3), both before any sibling's pages
    /// are returned.
    pub(super) fn free_runtime_region_pages(
        &mut self,
        id: RuntimeRegion,
        from_cascade: Option<RuntimeRegion>,
    ) -> usize {
        self.free_region_set(&[id], from_cascade)
    }

    /// Free a **co-owned region group** as one unit — the runtime `FreeRegionGroup`. An
    /// externally-unique mutual reference cycle with no container parent has no owner
    /// among its members, because each owns and is owned by the others. So it is
    /// reclaimed symmetrically. Every member is a root of the same wholesale drop,
    /// freed at the group's collective last use regardless of its reference count.
    ///
    /// Interior member↔member references resolve to unindexed siblings in phase 2 and
    /// are dropped. The cycle reclaims with the group, which per-region RC cannot do
    /// (docs/impl/region/rules.md Rule 8). Genuinely-Shared frontier references
    /// cascade once. Pinned by `region_ownership_reclaims_bare_cycle_group`
    /// (src/runtime/tests/ownership/subtree.rs).
    pub(crate) fn free_region_group(&mut self, members: &[RuntimeRegion]) -> usize {
        self.free_region_set(members, None)
    }

    /// The wholesale free shared by single-root subtree drop
    /// (`free_runtime_region_pages`) and co-owned group drop (`free_region_group`).
    /// `roots` seeds the first tear-down: one region for a subtree, the whole member
    /// set for a group.
    ///
    /// **Iterative cascade, not recursive.** Tearing a set down ([`Self::teardown_set`])
    /// yields a frontier of genuinely-Shared cross-region references to decref. A
    /// frontier reference that reaches rc 0 is itself a new set to tear down, whose
    /// frontier feeds the same loop. A heap worklist drives that, where native
    /// recursion would run `teardown → decref → free → teardown`. So a chain of N
    /// cross-region references frees in O(1) native stack instead of one frame per
    /// link: a long list, a deeply-nested structure, `(apply concat
    /// <thousands-of-chunks>)`. Pinned by `deep_cascade_chain_does_not_overflow_stack`
    /// (`regionstore::tests`) and `tests/impl/region-deep-chain.lisp`.
    ///
    /// The worklist and every set a tear-down fills are the store's `free_sets`,
    /// taken for the free and put back cleared, so a warm free allocates nothing.
    /// A free that starts inside this one, from a destructor, finds them taken and
    /// works in fresh sets.
    ///
    /// A cascaded free's set is a single Counted region that just hit rc 0. So
    /// re-tearing an already-unindexed root is a harmless no-op
    /// ([`Self::teardown_set`] phase 1 skips an absent slot). The rc bookkeeping
    /// guarantees each region is enqueued exactly once under balanced accounting.
    fn free_region_set(
        &mut self,
        roots: &[RuntimeRegion],
        from_cascade: Option<RuntimeRegion>,
    ) -> usize {
        let mut sets = std::mem::take(&mut self.free_sets);
        sets.cascade.clear();
        let mut freed = self.teardown_and_release(roots, from_cascade, &mut sets);
        while let Some((r, src)) = sets.cascade.pop() {
            freed += self.teardown_and_release(&[r], src, &mut sets);
        }
        self.free_sets = sets;
        freed
    }

    /// Tear `set` down, then decref each frontier reference it left, queueing a
    /// region that reached rc 0 on the cascade. `from_cascade` is the root of the
    /// set whose frontier released `set`, carried for the free-log label alone.
    fn teardown_and_release(
        &mut self,
        set: &[RuntimeRegion],
        from_cascade: Option<RuntimeRegion>,
        sets: &mut FreeSets,
    ) -> usize {
        let freed = self.teardown_set(set, from_cascade, sets);
        if sets.frontier.is_empty() {
            return freed;
        }
        // The cascade source is a representative root of the set just torn down
        // (tracing/label only — a group's members are co-equal).
        let cascade_src = set.first().copied();
        if crate::config::get().has_trace("rc") {
            eprintln!(
                "[trace:rc] free_region_set({set:?}) cascade: {:?}",
                sets.frontier
            );
        }
        for ref_id in sets.frontier.drain(..) {
            if let Some(r) = RuntimeRegion::new(ref_id) {
                // Decrement here (no free), and enqueue the region for tear-down only
                // if this reference was its last — the loop, not the native stack,
                // carries the cascade to the next link.
                if self.decref_reaches_zero(r, cascade_src) {
                    sets.cascade.push((r, cascade_src));
                }
            }
        }
        freed
    }

    /// Tear down one region set as a unit, answer the objects freed, and leave the
    /// cascade frontier in `sets.frontier`: the genuinely-Shared cross-region
    /// references (with multiplicity) the caller must decref. It derefs no freed
    /// page to produce them. This is phases 0–3 of the wholesale free.
    fn teardown_set(
        &mut self,
        roots: &[RuntimeRegion],
        from_cascade: Option<RuntimeRegion>,
        sets: &mut FreeSets,
    ) -> usize {
        // Phase 0 — collect the candidate dying set READ-ONLY: the roots plus their
        // transitive owned_children. The entries stay indexed, so the rescue below
        // can walk edges and owner chains. Then RESCUE any member that external
        // uniqueness does not hold for at this drop (docs/impl/region/ownership.md).
        // The `dying` set guards a member reachable through two owner edges
        // (defensive — the inference adopts each member once).
        //
        // Each stack entry carries the owner that listed it. A seeded root carries
        // `None`: a Counted region whose rc-0 free triggered this drop, or a
        // co-owned group member. So the walk can debug-assert that the forest's
        // forward and back edges agree: a region reached as a child must record
        // exactly that parent as its `Owned` owner.
        sets.frontier.clear();
        sets.candidates.clear();
        sets.dying.clear();
        sets.stack.clear();
        sets.stack.extend(roots.iter().map(|&r| (r, None)));
        while let Some((r, expected_owner)) = sets.stack.pop() {
            let idx = r.get() as usize;
            if idx >= self.regions.len() || !sets.dying.insert(r.get()) {
                continue;
            }
            if let Some(entry) = self.regions[idx].as_ref() {
                debug_assert!(
                    match (&entry.reclaim, expected_owner) {
                        // A child reached through `owned_children` must be Owned by
                        // exactly the parent that listed it (forward edge ⟺ back edge).
                        (Reclaim::Owned { owner }, Some(parent)) => *owner == parent,
                        // The root is the Counted region whose free triggered the drop.
                        (Reclaim::Counted(_), None) => true,
                        _ => false,
                    },
                    "owned-subtree edge inconsistency: region {r} reached as a child of \
                     {expected_owner:?}, but its reclaim mode does not record that owner \
                     (docs/impl/region/ownership.md)",
                );
                for &c in &entry.owned_children {
                    sets.stack.push((c, Some(r)));
                }
                sets.candidates.push((r, expected_owner));
            } else {
                sets.dying.remove(&r.get());
            }
        }
        self.rescue_externally_referenced(&sets.candidates, &mut sets.dying);
        // Phase 1 — unindex every still-dying member, taking its entry out of
        // `regions` (so `owning_region` resolves its pages to no region) while its
        // pages stay mapped — the moved-out entry still owns them.
        sets.members.clear();
        for &(r, _) in &sets.candidates {
            if !sets.dying.contains(&r.get()) {
                continue;
            }
            if let Some(mut entry) = self.regions[r.get() as usize].take() {
                entry.owned_children.clear();
                sets.members.push((r, entry));
            }
        }
        if sets.members.is_empty() {
            return 0;
        }
        // Phase 2 — build the cascade frontier from each member's RECORDED `outgoing`
        // edge table (docs/impl/region/ownership.md), NOT a content scan. A target
        // unindexed in phase 1 (interior to the freed set) is no longer live, and
        // is dropped: reclaimed with the set, never cascaded. A target outside is a
        // genuinely-Shared frontier ref, pushed once per recorded reference. No phase
        // derefs a heap page to discover an edge.
        //
        // Recorded edges carry only region ids, so the frontier filter is liveness
        // (a member unindexed in phase 1 fails it — interior, dropped). A dying
        // source's edges into live targets (rescued members included) are retired
        // from each target's `incoming` mirror right after the frontier is built —
        // the third maintenance site of the mirror (docs/impl/region/ownership.md).
        sets.retire.clear();
        for (r, entry) in &sets.members {
            for (&target, &count) in &entry.outgoing {
                let tidx = target.get() as usize;
                if tidx < self.regions.len() && self.regions[tidx].is_some() {
                    for _ in 0..count {
                        sets.frontier.push(target.get());
                    }
                    sets.retire.push((*r, target, count));
                }
            }
        }
        for &(src, dst, count) in &sets.retire {
            self.unmirror_incoming(src, dst, count);
        }
        // The `#[cfg(debug_assertions)]` oracle asserts that the recorded table
        // matches a content scan of the same members. The scan derefs target pages
        // to classify them, so it runs here, while every member's pages are still
        // mapped. It resolves pointers by ownership (`owning_region`), the resolver
        // the alloc funnel recorded with, which keeps the two sides symmetric. Any
        // accounting drift then detonates at the free site.
        #[cfg(debug_assertions)]
        {
            let owner = |p| self.owning_region(p);
            sets.scanned.clear();
            for (r, entry) in &sets.members {
                entry
                    .pool
                    .find_region_cross_refs(r.get(), &owner, &mut sets.scanned);
            }
            sets.recorded.clear();
            sets.recorded.extend_from_slice(&sets.frontier);
            sets.recorded.sort_unstable();
            sets.scanned.sort_unstable();
            assert!(
                sets.recorded == sets.scanned,
                "outgoing-edge table drift freeing {roots:?}: recorded frontier \
                 {:?} != content scan {:?} — a missed store-funnel \
                 edge (a leak) or a double-record (a UAF) \
                 (docs/impl/region/ownership.md)",
                sets.recorded,
                sets.scanned,
            );
        }
        self.discharge_fibers(&sets.members, &mut sets.frontier);
        // Phase 3 — tear down every member's pages, bump its generation, and recycle
        // the now-free physical id (always ≥ 2, never a reserved id). After the bump,
        // a surviving pointer into the pages panics at its next debug-build
        // region_of, and the recycled id's next incarnation stamps fresh pages with
        // the bumped value. The emptied entry builds the next region.
        let mut freed = 0;
        for (r, mut entry) in sets.members.drain(..) {
            self.counters.count_free(
                entry.pool.page_count() as u64,
                entry.pool.obj_count() as u64,
                matches!(entry.reclaim, Reclaim::Owned { .. }),
            );
            if crate::value::fiberheap::freelog::enabled() {
                let kind = from_cascade.map_or("direct".to_string(), |s| format!("cascade({s})"));
                crate::value::fiberheap::freelog::record_free(
                    r.get(),
                    kind,
                    entry.pool.page_ranges(),
                );
            }
            freed += self.release_pages(r.get(), &mut entry);
            self.free_physical.push(r.get());
            self.keep_entry(entry);
        }
        // The genuinely-Shared frontier refs lie outside this subtree, untouched by the
        // teardown; the caller cascades them iteratively.
        freed
    }
}

mod discharge;
mod rescue;
mod sets;
pub(super) use sets::FreeSets;
