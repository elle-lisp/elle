use super::*;

impl RegionStore {
    /// Free the entire **owned subtree** rooted at `id` — `id` plus every region
    /// transitively reachable through `owned_children` — as a unit (subtree drop,
    /// docs/impl/region-model.md § "Adoption and subtree drop"). For a region with no
    /// owned children this is an ordinary single-region free; for an owner it frees
    /// the whole subtree, interior reference cycles included.
    ///
    /// **Phased, so the debug oracle never reads a freed sibling.** The production
    /// frontier comes from each member's recorded `outgoing` edge table
    /// (docs/impl/region-model.md § "The outgoing edge table"), so reclamation derefs
    /// **no** heap page. In debug builds an equivalence oracle additionally scans member
    /// contents (`find_region_cross_refs` → `region_of_page_ptr`, which derefs each
    /// pointer's page) and asserts it matches the table; that scan must run while all
    /// member pages are still mapped, which the phase split guarantees:
    ///
    /// 1. **Unindex** every member (take its entry out of `regions`, so `valid_region`
    ///    reports it absent) while its pages stay mapped — the moved-out entry still
    ///    owns them. Walking `owned_children` is Rust-side, so collecting the subtree
    ///    never derefs a heap page.
    /// 2. **Frontier** — walk every member's recorded `outgoing` table: a target still
    ///    indexed is a genuinely-**Shared** frontier ref to cascade; a target unindexed
    ///    in phase 1 is interior to the freed set and dropped (the cycle reclaims with
    ///    the set, never cascades). The debug oracle's content scan runs here too, while
    ///    all member pages are mapped, so an interior reference cycle (the
    ///    `(push a b)(push b a)` knot) never reads a sibling's returned page.
    /// 3. **Tear down** every member's pages and bump its generation (a later stale
    ///    deref detonates at the next debug `region_of`), recycling the id.
    /// 4. **Cascade** the collected Shared-frontier refs once — their pages lie outside
    ///    the subtree, untouched by the teardown.
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

    /// Free a **co-owned region group** as one unit — the runtime `FreeRegionGroup`.
    /// An externally-unique mutual reference
    /// cycle with no container parent has no owner among its members (each owns and is
    /// owned by the others), so it is reclaimed symmetrically: every member is a root of
    /// the same wholesale drop, freed at the group's collective last use regardless of its
    /// reference count. Interior member↔member references resolve to unindexed siblings in
    /// phase 2 and are dropped (the cycle reclaims with the group, which per-region RC
    /// cannot do — region-rules.md Rule 8); genuinely-Shared frontier references cascade
    /// once. Pinned by `free_region_group_reclaims_bare_cycle` (`regionstore::tests`).
    pub(crate) fn free_region_group(&mut self, members: &[RuntimeRegion]) -> usize {
        self.free_region_set(members, None)
    }

    /// The four-phase wholesale free shared by single-root subtree drop
    /// ([`free_runtime_region_pages`]) and co-owned group drop ([`free_region_group`]):
    /// `roots` seeds the collection — one region for a subtree, the whole member set for a
    /// group. The phase split (unindex all, then frontier-from-table, then tear down) is
    /// what lets the debug oracle's content scan never read a freed sibling; see the body
    /// comments.
    fn free_region_set(
        &mut self,
        roots: &[RuntimeRegion],
        from_cascade: Option<RuntimeRegion>,
    ) -> usize {
        // Phase 1 — collect and unindex the owned subtree (roots + transitive
        // owned_children). `seen` guards a member reachable through two owner edges
        // (defensive — the inference adopts each member once). Each entry on the
        // stack carries the owner that listed it (`None` for a seeded root — a Counted
        // region whose rc-0 free triggered this drop, or a co-owned group member), so the
        // walk can debug-assert the forest's forward/back edges agree: a region reached as
        // a child must record exactly that parent as its `Owned` owner.
        let mut members: Vec<(RuntimeRegion, RegionEntry)> = Vec::new();
        let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut stack: Vec<(RuntimeRegion, Option<RuntimeRegion>)> =
            roots.iter().map(|&r| (r, None)).collect();
        while let Some((r, expected_owner)) = stack.pop() {
            let idx = r.get() as usize;
            if idx >= self.regions.len() || !seen.insert(r.get()) {
                continue;
            }
            if let Some(mut entry) = self.regions[idx].take() {
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
                     (docs/impl/region-model.md § 'The runtime: a reclamation typestate')",
                );
                for &c in &entry.owned_children {
                    stack.push((c, Some(r)));
                }
                entry.owned_children.clear();
                members.push((r, entry));
            }
        }
        if members.is_empty() {
            return 0;
        }
        // Phase 2 — build the cascade frontier from each member's RECORDED `outgoing`
        // edge table (docs/impl/region-model.md § "The outgoing edge table"), NOT a
        // content scan. A target unindexed in phase 1 (interior to the freed set) fails
        // `valid_region` and is dropped — reclaimed with the set, never cascaded; a
        // target outside is a genuinely-Shared frontier ref, pushed once per recorded
        // reference. No phase derefs a heap page to discover an edge. The
        // `#[cfg(debug_assertions)]` oracle then asserts the recorded table matches a
        // one-time content scan of the same members — run here, while every member's
        // pages are still mapped (the scan derefs target pages to classify them) — so
        // any accounting drift detonates at the free site.
        #[cfg(debug_assertions)]
        let page_size = self.pool.initial_page_size();
        let mut frontier: Vec<u32> = Vec::new();
        {
            // Recorded edges carry only region ids, so the frontier filter is
            // liveness (a member unindexed in phase 1 fails it — interior,
            // dropped). The oracle's content scan below re-derives edges from
            // POINTERS, so its filter is the full OWNERSHIP predicate
            // (`find_object_cross_refs`) — the same one the alloc funnel
            // recorded with, keeping the two sides symmetric.
            let live_region = |rid: u32| -> bool {
                let ridx = rid as usize;
                ridx < self.regions.len() && self.regions[ridx].is_some()
            };
            for (_r, entry) in &members {
                for (&target, &count) in &entry.outgoing {
                    if live_region(target.get()) {
                        for _ in 0..count {
                            frontier.push(target.get());
                        }
                    }
                }
            }
            #[cfg(debug_assertions)]
            {
                let owning_region = |rid: u32, ptr: *const ()| -> bool {
                    self.regions
                        .get(rid as usize)
                        .and_then(|s| s.as_ref())
                        .is_some_and(|e| e.pool.owns(ptr))
                };
                let mut scanned: Vec<u32> = Vec::new();
                for (r, entry) in &members {
                    scanned.extend(entry.pool.find_region_cross_refs(
                        r.get(),
                        page_size,
                        &owning_region,
                    ));
                }
                let mut recorded = frontier.clone();
                recorded.sort_unstable();
                scanned.sort_unstable();
                assert!(
                    recorded == scanned,
                    "outgoing-edge table drift freeing {roots:?}: recorded frontier \
                     {recorded:?} != content scan {scanned:?} — a missed store-funnel \
                     edge (a leak) or a double-record (a UAF) \
                     (docs/impl/region-model.md § 'The outgoing edge table')",
                );
            }
        }
        // Phase 3 — tear down every member's pages, bump its generation (a surviving
        // pointer into them panics at its next debug-build region_of, and the recycled
        // id's next incarnation stamps fresh pages with the bumped value), and recycle
        // the now-free physical id (always ≥ 2, never a reserved id).
        let mut freed = 0;
        for (r, mut entry) in members {
            if crate::value::fiberheap::freelog::enabled() {
                let kind = from_cascade.map_or("direct".to_string(), |s| format!("cascade({s})"));
                crate::value::fiberheap::freelog::record_free(
                    r.get(),
                    kind,
                    entry.pool.page_ranges(),
                );
            }
            freed += entry.pool.teardown(&mut self.pool);
            self.bump_generation(r.get());
            self.free_physical.push(r.get());
        }
        // Phase 4 — cascade the genuinely-Shared frontier refs once. The cascade source
        // is a representative root (tracing only — a group's members are co-equal).
        let cascade_src = roots.first().copied();
        if crate::config::get().has_trace("rc") && !frontier.is_empty() {
            eprintln!("[trace:rc] free_region_set({roots:?}) cascade: {frontier:?}");
        }
        for ref_id in frontier {
            if let Some(r) = RuntimeRegion::new(ref_id) {
                self.decref_with_cascade(r, cascade_src);
            }
        }
        freed
    }
}
