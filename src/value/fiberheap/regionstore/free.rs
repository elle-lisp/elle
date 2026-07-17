use super::*;

impl RegionStore {
    /// Free the entire **owned subtree** rooted at `id` — `id` plus every region
    /// transitively reachable through `owned_children` — as a unit (subtree drop,
    /// docs/impl/region/ownership.md § "Adoption and subtree drop"). For a region with no
    /// owned children this is an ordinary single-region free; for an owner it frees
    /// the whole subtree, interior reference cycles included.
    ///
    /// **Phased, so the debug oracle never reads a freed sibling.** The production
    /// frontier comes from each member's recorded `outgoing` edge table
    /// (docs/impl/region/ownership.md § "The outgoing edge table"), so reclamation derefs
    /// **no** heap page. In debug builds an equivalence oracle additionally scans member
    /// contents (`find_region_cross_refs` → `region_of_page_ptr`, which derefs each
    /// pointer's page) and asserts it matches the table; that scan must run while all
    /// member pages are still mapped, which the phase split guarantees:
    ///
    /// 0. **Collect and rescue** — walk the subtree read-only, then convert to
    ///    `Counted` any member still externally referenced per the recorded edge
    ///    tables, pruning it (with its own subtree) from the dying set
    ///    (docs/impl/region/ownership.md § "The incoming edge table and the
    ///    external-reference rescue"). Walking `owned_children` is Rust-side, so
    ///    collecting the subtree never derefs a heap page.
    /// 1. **Unindex** every still-dying member (take its entry out of `regions`, so
    ///    `valid_region` reports it absent) while its pages stay mapped — the
    ///    moved-out entry still owns them.
    /// 2. **Frontier** — walk every member's recorded `outgoing` table: a target still
    ///    indexed is a genuinely-**Shared** frontier ref to cascade; a target unindexed
    ///    in phase 1 is interior to the freed set and dropped (the cycle reclaims with
    ///    the set, never cascades). The debug oracle's content scan runs here too, while
    ///    all member pages are mapped, so an interior reference cycle (the
    ///    `(push a b)(push b a)` knot) never reads a sibling's returned page.
    /// 3. **Tear down** every member's pages and bump its generation (a later stale
    ///    deref detonates at the next debug `region_of`), recycling the id.
    ///
    /// Steps 0–3 are [`Self::teardown_set`], which returns the Shared frontier rather than
    /// freeing it. [`Self::free_region_set`] then **cascades** those refs iteratively — a
    /// frontier ref that reaches rc 0 becomes another set fed back through the same
    /// steps — so a deep chain of cross-region references frees in O(1) native stack (their
    /// pages lie outside the subtree, untouched by the teardown).
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
    /// cannot do — region/rules.md Rule 8); genuinely-Shared frontier references cascade
    /// once. Pinned by `free_region_group_reclaims_bare_cycle` (`regionstore::tests`).
    pub(crate) fn free_region_group(&mut self, members: &[RuntimeRegion]) -> usize {
        self.free_region_set(members, None)
    }

    /// The wholesale free shared by single-root subtree drop
    /// (`free_runtime_region_pages`) and co-owned group drop (`free_region_group`):
    /// `roots` seeds the first tear-down — one region for a subtree, the whole member set
    /// for a group.
    ///
    /// **Iterative cascade, not recursive.** Tearing a set down ([`Self::teardown_set`])
    /// yields a frontier of genuinely-Shared cross-region references to decref; a frontier
    /// reference that reaches rc 0 is itself a new set to tear down, whose frontier feeds
    /// the same loop. Driving that with a heap worklist (rather than
    /// `teardown → decref → free → teardown` native recursion) frees a chain of N
    /// cross-region references — a long list, a deeply-nested structure, `(apply concat
    /// <thousands-of-chunks>)` — in O(1) native stack instead of one frame per link.
    /// Pinned by `deep_cascade_chain_does_not_overflow_stack` (`regionstore::tests`) and
    /// `tests/elle/region-deep-chain.lisp`.
    ///
    /// A cascaded free's set is a single Counted region that just hit rc 0, so re-tearing
    /// an already-unindexed root is a harmless no-op ([`Self::teardown_set`] phase 1 skips
    /// an absent slot); the rc bookkeeping guarantees each region is enqueued exactly once
    /// under balanced accounting.
    fn free_region_set(
        &mut self,
        roots: &[RuntimeRegion],
        from_cascade: Option<RuntimeRegion>,
    ) -> usize {
        let mut freed = 0;
        // Each item is a set to tear down and the cascade source that reached it
        // (`from_cascade`, carried only for the free-log label). The seed is the caller's
        // root set; every later item is a single frontier region that reached rc 0.
        let mut worklist: Vec<(Vec<RuntimeRegion>, Option<RuntimeRegion>)> =
            vec![(roots.to_vec(), from_cascade)];
        while let Some((set, fc)) = worklist.pop() {
            let (set_freed, frontier) = self.teardown_set(&set, fc);
            freed += set_freed;
            if frontier.is_empty() {
                continue;
            }
            // The cascade source is a representative root of the set just torn down
            // (tracing/label only — a group's members are co-equal).
            let cascade_src = set.first().copied();
            if crate::config::get().has_trace("rc") {
                eprintln!("[trace:rc] free_region_set({set:?}) cascade: {frontier:?}");
            }
            for ref_id in frontier {
                if let Some(r) = RuntimeRegion::new(ref_id) {
                    // Decrement here (no free), and enqueue the region for tear-down only
                    // if this reference was its last — the loop, not the native stack,
                    // carries the cascade to the next link.
                    if self.decref_reaches_zero(r, cascade_src) {
                        worklist.push((vec![r], cascade_src));
                    }
                }
            }
        }
        freed
    }

    /// Tear down one region set as a unit and return `(objects freed, cascade frontier)`.
    /// The frontier is the list of genuinely-Shared cross-region references (with
    /// multiplicity) the caller must decref; it derefs no freed page to produce it. This
    /// is phases 0–3 of the wholesale free — the free itself of the frontier is the
    /// caller's iterative concern ([`Self::free_region_set`]).
    fn teardown_set(
        &mut self,
        roots: &[RuntimeRegion],
        from_cascade: Option<RuntimeRegion>,
    ) -> (usize, Vec<u32>) {
        // Phase 0 — collect the candidate dying set READ-ONLY (roots + transitive
        // owned_children; the entries stay indexed so the rescue below can walk
        // edges and owner chains), then RESCUE any member external uniqueness does
        // not hold for at this drop (docs/impl/region/ownership.md § "The incoming
        // edge table and the external-reference rescue"). The `dying` set guards a
        // member reachable through two owner edges (defensive — the inference
        // adopts each member once). Each stack entry carries the owner that listed
        // it (`None` for a seeded root — a Counted region whose rc-0 free
        // triggered this drop, or a co-owned group member), so the walk can
        // debug-assert the forest's forward/back edges agree: a region reached as
        // a child must record exactly that parent as its `Owned` owner.
        let mut candidates: Vec<(RuntimeRegion, Option<RuntimeRegion>)> = Vec::new();
        let mut dying: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut stack: Vec<(RuntimeRegion, Option<RuntimeRegion>)> =
            roots.iter().map(|&r| (r, None)).collect();
        while let Some((r, expected_owner)) = stack.pop() {
            let idx = r.get() as usize;
            if idx >= self.regions.len() || !dying.insert(r.get()) {
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
                     (docs/impl/region/ownership.md § 'The runtime: a reclamation typestate')",
                );
                for &c in &entry.owned_children {
                    stack.push((c, Some(r)));
                }
                candidates.push((r, expected_owner));
            } else {
                dying.remove(&r.get());
            }
        }
        self.rescue_externally_referenced(&candidates, &mut dying);
        // Phase 1 — unindex every still-dying member, taking its entry out of
        // `regions` (so `valid_region` reports it absent) while its pages stay
        // mapped — the moved-out entry still owns them.
        let mut members: Vec<(RuntimeRegion, RegionEntry)> = Vec::new();
        for &(r, _) in &candidates {
            if !dying.contains(&r.get()) {
                continue;
            }
            if let Some(mut entry) = self.regions[r.get() as usize].take() {
                entry.owned_children.clear();
                members.push((r, entry));
            }
        }
        if members.is_empty() {
            return (0, Vec::new());
        }
        // Phase 2 — build the cascade frontier from each member's RECORDED `outgoing`
        // edge table (docs/impl/region/ownership.md § "The outgoing edge table"), NOT a
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
            // A dying source's edges into live targets (rescued members included)
            // are retired from each target's `incoming` mirror right after the
            // frontier is built — the third maintenance site of the mirror
            // (§ "The incoming edge table and the external-reference rescue").
            let mut retire: Vec<(RuntimeRegion, RuntimeRegion, u32)> = Vec::new();
            let live_region = |rid: u32| -> bool {
                let ridx = rid as usize;
                ridx < self.regions.len() && self.regions[ridx].is_some()
            };
            for (r, entry) in &members {
                for (&target, &count) in &entry.outgoing {
                    if live_region(target.get()) {
                        for _ in 0..count {
                            frontier.push(target.get());
                        }
                        retire.push((*r, target, count));
                    }
                }
            }
            for (src, dst, count) in retire {
                self.unmirror_incoming(src, dst, count);
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
                     (docs/impl/region/ownership.md § 'The outgoing edge table')",
                );
            }
        }
        // Phase 2b — the fiber discharge (docs/impl/region/owner.md § "The
        // free-path fiber discharge"): a dying `Fiber` object whose
        // fiber was never routed through a terminal transition (a dropped handle,
        // a still-paused or `:error` fiber) still holds its parked chain's
        // activation owner nodes, its own fiber node, and a parked non-terminal
        // signal's park escape retain. Take that state out of each dying fiber
        // and feed the regions to the same iterative cascade as the recorded
        // frontier. Runs AFTER the equivalence oracle: these are not content
        // edges (no record exists), so they must not participate in the
        // recorded == scanned comparison. A borrowed (executing) fiber is
        // skipped — its region cannot be dying while it runs.
        {
            let page_size = self.pool.initial_page_size();
            let mut discharged: Vec<u32> = Vec::new();
            for (_, entry) in &members {
                for obj in entry.pool.live_objects() {
                    let HeapObject::Fiber { handle, .. } = obj else {
                        continue;
                    };
                    handle.try_with_mut(|fib| {
                        let parked = fib.take_parked_state();
                        discharged.extend(parked.nodes.iter().map(|r| r.get()));
                        if let Some(node) = fib.fiber_owner_node.take() {
                            discharged.push(node.get());
                        }
                        // The parked non-terminal signal's park escape retain
                        // (EmitEscape / SuspendEscape), released at a resume
                        // that will never come. Resolve the value's region
                        // exactly as the content scan does (page-header read +
                        // ownership check); a foreign or immediate value
                        // resolves to nothing.
                        if let Some((_, v)) = parked.signal {
                            if let Some(ptr) = v.as_heap_ptr() {
                                let rid = unsafe {
                                    crate::value::fiberheap::regionpool::region_of_page_ptr(
                                        ptr, page_size,
                                    )
                                };
                                let owned_here = self
                                    .regions
                                    .get(rid as usize)
                                    .and_then(|s| s.as_ref())
                                    .is_some_and(|e| e.pool.owns(ptr));
                                if rid > 1 && owned_here {
                                    discharged.push(rid);
                                }
                            }
                        }
                    });
                }
            }
            if crate::config::get().has_trace("rc") && !discharged.is_empty() {
                eprintln!("[trace:rc] fiber_discharge → {discharged:?}");
            }
            frontier.extend(discharged);
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
        // The genuinely-Shared frontier refs lie outside this subtree, untouched by the
        // teardown; the caller cascades them iteratively.
        (freed, frontier)
    }

    /// Enforce external uniqueness **at the drop**: prune from `dying` every
    /// non-root member still referenced by a source that survives this drop,
    /// converting it `Owned → Counted` with a count rebuilt from its recorded
    /// incoming edges (docs/impl/region/ownership.md § "The incoming edge table
    /// and the external-reference rescue"). The rescued member's own subtree
    /// stays intact beneath it; every remaining referencer then releases it
    /// through the ordinary cascade, and the last release frees it.
    ///
    /// Rescue iterates to a fixpoint: a rescued member survives the drop, so the
    /// members *it* references become externally referenced and are rescued too —
    /// tearing one down would strand the survivor's live edge into it.
    ///
    /// The rebuilt count admits every incoming edge except those from the
    /// member's own surviving subtree: a dying source's share is consumed by its
    /// frontier decrefs in this same drop, a surviving source's at its own
    /// release, while an own-subtree back-edge releases only at the member's own
    /// drop and counting it would self-sustain the count (the member would leak).
    fn rescue_externally_referenced(
        &mut self,
        candidates: &[(RuntimeRegion, Option<RuntimeRegion>)],
        dying: &mut std::collections::HashSet<u32>,
    ) {
        let is_live = |regions: &Vec<Option<RegionEntry>>, id: u32| -> bool {
            (id as usize) < regions.len() && regions[id as usize].is_some()
        };
        // Fixpoint over the shrinking dying set. `rescued` keeps the owner that
        // listed each member so the unlink below detaches exactly that edge.
        let mut rescued: Vec<(RuntimeRegion, RuntimeRegion)> = Vec::new();
        loop {
            let mut changed = false;
            for &(r, owner) in candidates {
                // A seeded root is never rescued: it is Counted and reached its
                // own demise (rc 0, or a co-owned group's collective last use).
                let Some(owner) = owner else { continue };
                if !dying.contains(&r.get()) {
                    continue;
                }
                let Some(entry) = self.regions[r.get() as usize].as_ref() else {
                    continue;
                };
                if entry.incoming.is_empty() {
                    continue;
                }
                let externally_referenced = entry
                    .incoming
                    .keys()
                    .any(|s| !dying.contains(&s.get()) && is_live(&self.regions, s.get()));
                if !externally_referenced {
                    continue;
                }
                // The member and its whole owned subtree survive this drop.
                let mut stack = vec![r];
                while let Some(m) = stack.pop() {
                    if !dying.remove(&m.get()) {
                        continue;
                    }
                    if let Some(e) = self.regions[m.get() as usize].as_ref() {
                        stack.extend(e.owned_children.iter().copied());
                    }
                }
                rescued.push((r, owner));
                changed = true;
            }
            if !changed {
                break;
            }
        }
        if rescued.is_empty() {
            return;
        }
        // Unlink every rescued member from its owner FIRST: an owner that is
        // itself rescued survives, and must not re-claim the member at its own
        // later drop — and the rebuilt-count subtree walks below must see the
        // post-rescue forest (a rescued descendant is no longer "own subtree",
        // so its back-edge is a real counted reference).
        for &(r, owner) in &rescued {
            if let Some(o) = self
                .regions
                .get_mut(owner.get() as usize)
                .and_then(|s| s.as_mut())
            {
                o.owned_children.retain(|&c| c != r);
            }
        }
        for &(r, _) in &rescued {
            let mut subtree: std::collections::HashSet<u32> = std::collections::HashSet::new();
            let mut stack = vec![r];
            while let Some(m) = stack.pop() {
                if !subtree.insert(m.get()) {
                    continue;
                }
                if let Some(e) = self.regions[m.get() as usize].as_ref() {
                    stack.extend(e.owned_children.iter().copied());
                }
            }
            let entry = self.regions[r.get() as usize]
                .as_ref()
                .expect("a rescued member stays indexed through the rescue");
            let rc: u32 = entry
                .incoming
                .iter()
                .filter(|(s, _)| !subtree.contains(&s.get()))
                .map(|(_, &n)| n)
                .sum();
            debug_assert!(
                rc > 0,
                "region {r} rescued with no admissible incoming reference — the \
                 rescue trigger names a surviving source, so at least its edge \
                 must be admitted (docs/impl/region/ownership.md § 'The incoming \
                 edge table and the external-reference rescue')",
            );
            if crate::config::get().has_trace("rc") {
                eprintln!("[trace:rc] rescue({r}) externally referenced → Counted({rc})");
            }
            self.regions[r.get() as usize].as_mut().unwrap().reclaim = Reclaim::Counted(rc);
        }
    }
}
