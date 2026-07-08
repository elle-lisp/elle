use super::*;

impl RegionStore {
    /// Open a closed-scope mint log (docs/impl/region/rules.md § "Macro
    /// expansion — a closed allocation scope"). Every region minted until the
    /// matching [`reclaim_mint_scope`] is recorded. The scope does not nest:
    /// macro transformer bodies are compiled code that does not re-enter the
    /// expander, so a transformer call mints no nested scope.
    pub fn begin_mint_log(&mut self) {
        debug_assert!(
            self.mint_log.is_none(),
            "nested mint log — a closed allocation scope was opened inside another \
             (a macro transformer re-entered macro expansion?)"
        );
        self.mint_log = Some(Vec::new());
    }

    /// Close the scope opened by [`begin_mint_log`] and reclaim its dead
    /// scratch by RC. For every region minted in the scope that is still live
    /// (its generation still matches the mint record — a recycled id whose
    /// region was already freed mid-scope is skipped), balance its
    /// **unexplained** references: `rc − in_degree`, where `in_degree` is the
    /// number of references it receives from the contents of the *other
    /// survivors* (the internal edges of the scratch DAG; see the body comment
    /// for why scanning only the scope, not the whole heap, is both fast and
    /// sufficient). That is exactly the owner references the scope never released
    /// (the residue-census quantity); decreffing them lets the ordinary cascade
    /// (Rule 7) reclaim the immutable scratch DAG. A region whose RC is fully
    /// explained by survivor edges has `rc − in_degree == 0` and is left
    /// untouched, so this is RC-driven, not a blanket free.
    ///
    /// `protected` lists regions that must NOT be reclaimed even though they
    /// were minted in the scope: process-lifetime roots (trait method tables
    /// via `alloc_root`, the root region) whose single owner is held Rust-side
    /// (the registry), invisible to the heap-content in-degree scan. A
    /// transformer triggers the first such allocation when it dispatches a
    /// trait method (e.g. `append`'s `empty?`), so the root region can land in
    /// the mint log; reclaiming it would free the trait tables under the running
    /// program. Returns the number of objects reclaimed (for `alloc_count`).
    pub fn reclaim_mint_scope(&mut self, protected: &[RuntimeRegion]) -> usize {
        let log = match self.mint_log.take() {
            Some(l) => l,
            None => return 0,
        };
        let protected_set: std::collections::HashSet<u32> =
            protected.iter().map(|r| r.get()).collect();
        // Survivors: minted ids still naming the region we minted (generation
        // match), excluding process-lifetime roots. Dedup — a live id appears
        // once, but a freed-and-reminted id appears under two generations and
        // only the current one survives.
        let mut surv_set: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut survivors: Vec<(u32, u32)> = Vec::new();
        for (id, gen) in log {
            let idx = id as usize;
            let live = idx < self.regions.len() && self.regions[idx].is_some();
            let gen_ok = self.generations.get(idx).copied().unwrap_or(0) == gen;
            if live && gen_ok && !protected_set.contains(&id) && surv_set.insert(id) {
                survivors.push((id, gen));
            }
        }
        if survivors.is_empty() {
            return 0;
        }
        // In-degree into each survivor from the contents of the OTHER survivors
        // (the internal edges of the scratch DAG). Scanning only the survivor
        // set — not every live region — keeps reclaim O(scratch) per expansion;
        // over a stdlib load (thousands of expansions) the all-regions scan was
        // the difference between sub-second and tens of seconds. This omits an
        // edge from a *non-scratch* live region into the scratch (a persistent
        // cell holding a freshly-built value): a macro transformer is a pure
        // compile-time constructor that performs no such runtime mutation, and
        // the one class of in-scope allocation that IS held externally — the
        // process-lifetime roots (trait tables / root region, held Rust-side) —
        // is already excluded above. So every surviving scratch region's RC is
        // explained by survivor edges plus the owner references the transformer
        // never released, and `rc − internal_in_degree` is exactly the latter.
        let page_size = self.pool.initial_page_size();
        // Ownership-verified (`find_object_cross_refs`): a foreign pointer whose
        // masked header bytes collide with a survivor id must not inflate that
        // survivor's in-degree.
        let in_scope = |rid: u32, ptr: *const ()| -> bool {
            surv_set.contains(&rid)
                && self
                    .regions
                    .get(rid as usize)
                    .and_then(|s| s.as_ref())
                    .is_some_and(|e| e.pool.owns(ptr))
        };
        let mut indeg: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        for &(id, _gen) in &survivors {
            if let Some(e) = self.regions[id as usize].as_ref() {
                for to in e.pool.find_region_cross_refs(id, page_size, &in_scope) {
                    *indeg.entry(to).or_insert(0) += 1;
                }
            }
        }
        // Escape count per survivor, snapshotted from the pre-reclaim RC. Taken
        // before any decref so a cascade that reduces one survivor's RC mid-pass
        // does not shrink another's target: across the DAG the decrefs issued
        // here (Σ escape) plus the cascades they trigger (Σ internal edges) sum
        // to each region's full RC, so all dead scratch reaches zero.
        let escape: std::collections::HashMap<u32, u32> = survivors
            .iter()
            .map(|&(id, _)| {
                // `count()` is 0 for an `Owned` survivor — its reclamation is the
                // owner's subtree drop, not this scope's RC balance, so it is never
                // decref'd here (escape saturates to 0). For `Counted` it is the
                // live count, exactly as before.
                let rc = self.regions[id as usize].as_ref().map_or(0, |e| e.count());
                (id, rc.saturating_sub(indeg.get(&id).copied().unwrap_or(0)))
            })
            .collect();
        // Balance each survivor's unexplained references. Cascades (Rule 7) may
        // free a survivor before this loop reaches it; the per-decref
        // liveness+generation recheck skips an already-reclaimed id (and stops
        // once a region's own escape decrefs free it), so nothing is double-freed.
        let mut freed_objs = 0;
        for (id, gen) in survivors {
            let idx = id as usize;
            for _ in 0..escape.get(&id).copied().unwrap_or(0) {
                let live = idx < self.regions.len() && self.regions[idx].is_some();
                let gen_ok = self.generations.get(idx).copied().unwrap_or(0) == gen;
                if !(live && gen_ok) {
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
