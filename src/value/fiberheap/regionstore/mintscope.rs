// audited: 2026-11-25
//! The closed allocation scope of a macro expansion: its mint log, and the reclaim that balances each survivor's references.
//!
//! docs/impl/region/macroscope.md

use super::*;

impl RegionStore {
    /// Open a closed-scope mint log (docs/impl/region/macroscope.md). Every
    /// region minted until the matching `reclaim_mint_scope` is recorded in it.
    /// A scope nests: a transformer's `eval` expands macros inside the
    /// transformer's own scope, and a mint records into the innermost log.
    pub fn begin_mint_log(&mut self) {
        self.mint_logs.push(Vec::new());
    }

    /// Record that a Rust structure holds one of `id`'s counted references,
    /// which a reclaim then counts as explained.
    pub fn note_rust_hold(&mut self, id: u32) {
        *self.rust_holds.entry(id).or_insert(0) += 1;
    }

    /// Forget one Rust-held reference to `id`, before its holder releases it.
    pub fn drop_rust_hold(&mut self, id: u32) {
        let held = self
            .rust_holds
            .get_mut(&id)
            .expect("a released Rust hold was noted when it was taken");
        *held -= 1;
        if *held == 0 {
            self.rust_holds.remove(&id);
        }
    }

    /// Whether the region `id` names is still the one minted at generation
    /// `gen`.
    fn minted_live(&self, id: u32, gen: u32) -> bool {
        let idx = id as usize;
        idx < self.regions.len()
            && self.regions[idx].is_some()
            && self.generations.get(idx).copied().unwrap_or(0) == gen
    }

    /// Close the scope opened by `begin_mint_log` and reclaim its dead
    /// scratch by RC. The pass visits every region minted in the scope that is
    /// still live: its generation still matches the mint record, so a recycled
    /// id whose region was already freed mid-scope is skipped. It balances each
    /// such region's **unexplained** references, `rc − in_degree − held`.
    /// `in_degree` counts the references the region receives from the contents
    /// of the *other survivors*, the internal edges of the scratch DAG, and
    /// `held` the references a Rust structure holds (`note_rust_hold`). The body
    /// comment argues why scanning only the scope, not the whole heap, is both
    /// fast and sufficient.
    ///
    /// The unexplained references are exactly the owner references the scope
    /// never released (the residue-census quantity). Decreffing them lets the
    /// ordinary cascade (Rule 7) reclaim the immutable scratch DAG. A region
    /// whose RC is fully explained by survivor edges has `rc − in_degree == 0`
    /// and is left untouched, so this is RC-driven, not a blanket free.
    ///
    /// `protected` lists regions that must NOT be reclaimed even though they
    /// were minted in the scope. They are the process roots, the root region,
    /// and every code region a `CodeUnit` or `CodePin` holds. Each owner is
    /// held Rust-side, invisible to the heap-content in-degree scan. A
    /// transformer allocates the first trait method table into the root region
    /// when it dispatches a trait method (for example `append`'s `empty?`), so
    /// the root region can land in the mint log. Reclaiming it would free the trait tables under the running
    /// program. Returns the number of objects reclaimed (for `alloc_count`).
    ///
    /// The scope closed is the innermost. A region its reclaim leaves live
    /// joins the enclosing scope's log, whose reclaim balances it again.
    pub fn reclaim_mint_scope(&mut self, protected: &[RuntimeRegion]) -> usize {
        let Some(log) = self.mint_logs.pop() else {
            return 0;
        };
        let freed = self.reclaim_log(&log, protected);
        let left: Vec<(u32, u32)> = log
            .into_iter()
            .filter(|&(id, gen)| self.minted_live(id, gen))
            .collect();
        if let Some(enclosing) = self.mint_logs.last_mut() {
            enclosing.extend(left);
        }
        freed
    }

    /// Balance the unexplained references of every region `log` minted that
    /// is still live, as [`reclaim_mint_scope`](Self::reclaim_mint_scope)
    /// states.
    fn reclaim_log(&mut self, log: &[(u32, u32)], protected: &[RuntimeRegion]) -> usize {
        let protected_set: std::collections::HashSet<u32> =
            protected.iter().map(|r| r.get()).collect();
        // Survivors: minted ids still naming the region we minted (generation
        // match), excluding process-lifetime roots. Dedup — a live id appears
        // once, but a freed-and-reminted id appears under two generations and
        // only the current one survives.
        let mut surv_set: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut survivors: Vec<(u32, u32)> = Vec::new();
        for &(id, gen) in log {
            if self.minted_live(id, gen) && !protected_set.contains(&id) && surv_set.insert(id) {
                survivors.push((id, gen));
            }
        }
        if survivors.is_empty() {
            return 0;
        }
        // In-degree into each survivor from the contents of the OTHER survivors
        // (the internal edges of the scratch DAG). Scanning only the survivor
        // set, not every live region, keeps reclaim O(scratch) per expansion. A
        // stdlib load runs thousands of expansions, and an all-regions scan per
        // expansion turns a sub-second load into tens of seconds.
        //
        // This omits an edge from a *non-scratch* live region into the scratch:
        // a persistent cell holding a freshly-built value. A macro transformer is
        // a pure compile-time constructor that performs no such runtime
        // mutation. The in-scope allocations that ARE held externally, the
        // regions a Rust-side owner holds, are already excluded above. So every
        // surviving scratch region's RC is explained by survivor edges plus the
        // owner references the transformer never released, and
        // `rc − internal_in_degree` is exactly the latter.
        // Resolved by ownership (`owning_region`): a foreign pointer whose
        // masked header bytes collide with a survivor id must not inflate that
        // survivor's in-degree.
        let in_scope = |ptr: *const ()| -> u32 {
            let rid = self.owning_region(ptr);
            if surv_set.contains(&rid) {
                rid
            } else {
                0
            }
        };
        let mut indeg: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        let mut refs = Vec::new();
        for &(id, _gen) in &survivors {
            if let Some(e) = self.regions[id as usize].as_ref() {
                refs.clear();
                e.pool.find_region_cross_refs(id, &in_scope, &mut refs);
                for &to in &refs {
                    *indeg.entry(to).or_insert(0) += 1;
                }
            }
        }
        // Escape count per survivor, snapshotted from the pre-reclaim RC. It is
        // taken before any decref, so a cascade that reduces one survivor's RC
        // mid-pass does not shrink another's target. Across the DAG the decrefs
        // issued here (Σ escape) plus the cascades they trigger (Σ internal
        // edges) sum to each region's full RC, so all dead scratch reaches zero.
        let escape: std::collections::HashMap<u32, u32> = survivors
            .iter()
            .map(|&(id, _)| {
                // `count()` is 0 for an `Owned` survivor. Its reclamation is the
                // owner's subtree drop, not this scope's RC balance, so it is
                // never decref'd here (escape saturates to 0). For `Counted` it
                // is the live count.
                let rc = self.regions[id as usize].as_ref().map_or(0, |e| e.count());
                let explained = indeg.get(&id).copied().unwrap_or(0)
                    + self.rust_holds.get(&id).copied().unwrap_or(0);
                (id, rc.saturating_sub(explained))
            })
            .collect();
        // Balance each survivor's unexplained references. Cascades (Rule 7) may
        // free a survivor before this loop reaches it. The per-decref liveness
        // and generation recheck skips an already-reclaimed id, and stops once a
        // region's own escape decrefs free it, so nothing is double-freed.
        let mut freed_objs = 0;
        for (id, gen) in survivors {
            for _ in 0..escape.get(&id).copied().unwrap_or(0) {
                if !self.minted_live(id, gen) {
                    break;
                }
                if let Some(r) = RuntimeRegion::new(id) {
                    freed_objs += self.decref_with_cascade(r, None);
                }
            }
        }
        freed_objs
    }
}

/// The heap's surface over its store's mint scopes.
impl crate::value::FiberHeap {
    /// Open a mint scope around a macro expansion, as
    /// `RegionStore::begin_mint_log` does.
    pub fn begin_region_mint_log(&mut self) {
        self.region_store.begin_mint_log();
    }

    /// [`RegionStore::note_rust_hold`] for `region`.
    pub(crate) fn note_rust_hold(&mut self, region: RuntimeRegion) {
        self.region_store.note_rust_hold(region.get());
    }

    /// [`RegionStore::drop_rust_hold`] for `region`.
    pub(crate) fn drop_rust_hold(&mut self, region: RuntimeRegion) {
        self.region_store.drop_rust_hold(region.get());
    }

    /// Close the innermost mint scope, as `RegionStore::reclaim_mint_scope`
    /// does, and take the objects it reclaimed off `alloc_count`.
    pub fn reclaim_region_mint_scope(&mut self, protected: &[RuntimeRegion]) {
        let freed = self.region_store.reclaim_mint_scope(protected);
        self.alloc_count -= freed;
    }
}
