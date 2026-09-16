// audited: 2026-09-15
//! The phantom placeholder regions: a value with no compiled allocation to
//! name still needs a release, so it is given a region of its own.
//!
//! docs/impl/region/cells.md
//! docs/impl/region/anchors.md

use super::*;

/// The name a rest sub-pattern binds to the collection ITSELF, where a bare
/// name matched the rest. `None` where a further pattern did, whose names
/// project the collection rather than hold it.
fn bound_rest_name(rest: &HirPattern) -> Option<Binding> {
    match rest {
        HirPattern::Var(b) => Some(*b),
        _ => None,
    }
}

impl RegionInference {
    /// Give every rest name a `Match` arm binds to a BUILT collection a
    /// placeholder region of its own, and hand that region to the name
    /// (docs/impl/region/anchors.md § "A rest pattern's collection is built,
    /// not read out").
    ///
    /// A bare name and nothing else. A decision tree loads each name by walking
    /// its own access path and re-runs the build on every path through the
    /// rest, so a collection a further pattern matched has as many allocations
    /// as it has names and one placeholder would not cover them
    /// (docs/impl/region/anchors.md § "Where the holder set stops").
    pub(super) fn record_match_rest_regions<'p>(
        &mut self,
        node: HirId,
        patterns: impl Iterator<Item = &'p HirPattern>,
    ) {
        if self.pattern_rest_regions.contains_key(&node) {
            return;
        }
        let names: Vec<Binding> = patterns
            .flat_map(|p| p.allocating_rest_bindings())
            .collect();
        if names.is_empty() {
            return;
        }
        let mut recorded = Vec::with_capacity(names.len());
        for b in names {
            let r = self.mint_rest_region(&[b]);
            recorded.push(RestCollection::bound(r, b));
        }
        self.pattern_rest_regions.insert(node, recorded);
    }

    /// Give every collection a `Destructure`'s pattern BUILDS a placeholder
    /// region of its own, and hand that region to every name that reaches it.
    ///
    /// `lower_destructure` emits one build per rest sub-pattern, so one region
    /// per sub-pattern is one per allocation whether a bare name or a further
    /// pattern matched it. A sub-pattern that binds no name leaves the
    /// collection with nothing to key a route on and takes none.
    pub(super) fn record_destructure_rest_regions(&mut self, node: HirId, pattern: &HirPattern) {
        if self.pattern_rest_regions.contains_key(&node) {
            return;
        }
        let built: Vec<(Option<Binding>, Vec<Binding>)> = pattern
            .building_rests()
            .into_iter()
            .map(|rest| (bound_rest_name(rest), rest.rest_collection_holders()))
            .filter(|(_, holders)| !holders.is_empty())
            .collect();
        if built.is_empty() {
            return;
        }
        let mut recorded = Vec::with_capacity(built.len());
        for (name, holders) in built {
            let r = self.mint_rest_region(&holders);
            recorded.push(match name {
                Some(b) => RestCollection::bound(r, b),
                None => RestCollection::projected(r, holders),
            });
        }
        self.pattern_rest_regions.insert(node, recorded);
    }

    /// Mint the phantom placeholder for one built collection, and record it
    /// against every name that reaches the collection so the binding chain
    /// carries the release over each name's uses.
    ///
    /// The region is phantom — no `alloc_here`, because the opcode mints the
    /// physical region at runtime and no compiled allocation names a static
    /// slot for it — so it is filtered out of `live_regions` and records no
    /// cross-region edge, exactly as a lambda's owned parameter is. Membership
    /// of `call_result_regions` is what routes its release through the slot the
    /// lowerer parks the collection in.
    fn mint_rest_region(&mut self, holders: &[Binding]) -> Region {
        let r = self.fresh_region(self.current_region);
        self.call_result_regions.insert(r);
        for &b in holders {
            let entry = self.binding_regions.entry(b).or_default();
            if !entry.contains(&r) {
                entry.push(r);
            }
        }
        r
    }

    /// A captured (`needs_capture`) binding introduced INSIDE a lambda body is
    /// materialized as a per-value env cell by `populate_env` (a `StoreCapture`
    /// into a cell pre-allocated from `capture_locals_mask` — NOT a compiled
    /// `MakeCaptureCell`, which the lowerer emits only at top level / outside a
    /// lambda; see `lower_define`/`lower_let` `self.in_lambda` split). Such an
    /// env cell has no compiled `DecrefRegion`, so without a release its initial
    /// rc=1 leaks (docs/impl/region/rules.md Rule 8 — the env region needs an explicit release).
    ///
    /// Give it a phantom cell placeholder (no `alloc_here` → filtered from
    /// `live_regions`, so no spurious compile-time `IncrefRegion` edge) in
    /// `call_result_regions` + `cell_release_regions`, so the lowerer releases
    /// the CELL at the binding's last use via `LoadCaptureRaw` +
    /// `DecrefCellRegion` (`region_of` the cell, never unwrapping to the inner
    /// value's region). `decref_point` comes from the binding-chains post-pass
    /// over the binding's uses (`binding_regions[b] = [cell_r]`), then the
    /// env-cell hoist (`analyze::decref::cells::post_loop_placement`) lifts it
    /// to the outermost enclosing loop: the box is minted once per activation,
    /// so its release must fire once per activation — a binding-last-use
    /// release that sits inside a loop frees the box on iteration 1 and the
    /// next iteration reads the recycled cell (the env-cell-in-loop UAF;
    /// docs/impl/region/cells.md "Env cells in loops: release once per
    /// activation, not per iteration").
    /// This mirrors the captured-param treatment in the Lambda arm. Returns the
    /// placeholder region iff the binding is such an env cell; `None` for
    /// top-level captured defs (compiled `MakeCaptureCell`, already released)
    /// and non-captured locals.
    pub(super) fn env_cell_placeholder(&mut self, binding: Binding) -> Option<Region> {
        // A binding whose forward cell is COMPILED holds that cell in its own
        // slot, so `populate_env` mints none for it and a placeholder here would
        // be a phantom — a `DecrefCellRegion` against a cell no allocation made.
        if self.compiled_cell_bindings.contains(&binding) {
            return None;
        }
        if self.in_lambda() && self.arena().get(binding).needs_capture() {
            // Idempotent per binding: a captured local is materialized as
            // EXACTLY ONE per-value CaptureCell (`populate_env`), released by a
            // single `DecrefCellRegion` at its last use (docs/impl/region/rules.md Rule 4
            // — "exactly once per activation"). `try_inline_call` re-walks an
            // inlined callee's body to discover cross-region edges, so a
            // captured local's `(var …)` Define inside a *nested* lambda (a
            // generator fiber body returned from an inlined function) can be
            // visited more than once. Minting a second cell-release region here
            // lowers to a second `DecrefCellRegion` for the one cell — a
            // double-free of the CaptureCell's region on resume
            // (region-fiber-capture-cell-resume-uaf.lisp). Reuse the cell
            // region already recorded for this binding instead.
            if let Some(regions) = self.binding_regions.get(&binding).cloned() {
                if let Some(existing) = regions
                    .into_iter()
                    .find(|r| self.cell_release_regions.contains(r))
                {
                    return Some(existing);
                }
            }
            let cell_r = self.fresh_region(self.current_region);
            self.call_result_regions.insert(cell_r);
            self.cell_release_regions.insert(cell_r);
            Some(cell_r)
        } else {
            None
        }
    }
}
