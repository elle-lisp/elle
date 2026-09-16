// audited: 2026-09-15
//! Region walk for the remaining HIR forms (`Match` through `Error`), split out
//! of `RegionInference::walk` to keep that dispatcher small.
//!
//! docs/impl/region/model.md

use super::*;

impl RegionInference {
    pub(super) fn walk_rest(&mut self, hir: &Hir) -> Vec<Region> {
        match &hir.kind {
            HirKind::Match { value, arms } => {
                // Register the Match node for pattern-level allocations
                // (ArrayMutSliceFrom, StructRest from destructuring).
                self.alloc_here(hir.id);
                let val_regions = self.walk(value);
                // Recorded before the arms are walked, so a body that reads a
                // rest name already sees its placeholder among the regions.
                self.record_match_rest_regions(hir.id, arms.iter().map(|(p, _, _)| p));
                let mut out = Vec::new();
                for (pat, guard, body) in arms {
                    for b in pat.bindings().bindings {
                        self.binding_region.insert(b, self.current_region);
                        // A pattern binding may ALIAS into the scrutinee's
                        // regions, so propagate them conservatively — the same
                        // reading the `Destructure` arm makes
                        // (docs/impl/region/model.md). Union, never overwrite:
                        // one arena `Binding` can be targeted by several
                        // matches, and overwriting drops an earlier source.
                        let entry = self.binding_regions.entry(b).or_default();
                        for r in val_regions.iter().copied() {
                            if !entry.contains(&r) {
                                entry.push(r);
                            }
                        }
                    }
                    if let Some(g) = guard {
                        let _ = self.walk(g);
                    }
                    out.extend(self.walk(body));
                }
                dedup_regions(&mut out);
                out
            }

            HirKind::And(exprs) | HirKind::Or(exprs) => {
                let mut out = Vec::new();
                for e in exprs {
                    out.extend(self.walk(e));
                }
                dedup_regions(&mut out);
                out
            }

            HirKind::Begin(exprs) => {
                // The predicate mirrors `lower_begin`: a `MakeCaptureCell` per
                // reachable `Define`/`Destructure` binding that takes a
                // COMPILED forward cell. An unconditional `alloc_here` would
                // mint phantom regions no runtime allocation pairs with.
                if self.begin_has_capturable_binding(exprs) {
                    // ONE region PER cell, never one for all of them
                    // (docs/impl/region/model.md). Recording each in
                    // `binding_regions[b]` is what lets the `decref_point`
                    // extension carry the cell over its binding's own uses,
                    // including uses in sibling top-level forms.
                    let mut capturable = Vec::new();
                    Self::collect_begin_capturable_bindings(
                        self.arena(),
                        self.in_lambda(),
                        exprs,
                        &mut capturable,
                    );
                    for b in capturable {
                        // A nested `Begin` reaches a binding the outer one
                        // already claimed, and the lowerer emits its cell once,
                        // so a second region here would be a phantom.
                        if self.compiled_cell_bindings.contains(&b) {
                            continue;
                        }
                        let cell_region = self.fresh_region(self.current_region);
                        self.record_compiled_cell(hir.id, b, cell_region);
                        let entry = self.binding_regions.entry(b).or_default();
                        if !entry.contains(&cell_region) {
                            entry.push(cell_region);
                        }
                    }
                }
                let mut last = Vec::new();
                for e in exprs {
                    last = self.walk(e);
                }
                last
            }

            HirKind::Block { block_id, body, .. } => {
                self.block_regions.insert(*block_id, self.current_region);

                let scope_region = self.fresh_region(self.current_region);
                self.scope_region.insert(hir.id, scope_region);
                let saved = self.current_region;
                self.current_region = scope_region;

                let mut last = Vec::new();
                for e in body {
                    last = self.walk(e);
                }

                self.current_region = saved;

                // The block's value is its fall-through value OR the value of
                // any `break` targeting it, so its result regions are the union
                // (docs/impl/region/anchors.md). Taken, not read: the entry
                // belongs to this block alone. The same regions go into
                // `break_sites` against THIS node, the transferring-node dual
                // of `return_sites`.
                if let Some(broken) = self.block_break_regions.remove(block_id) {
                    self.break_sites.push((hir.id, broken.clone()));
                    last.extend(broken);
                }
                // The break SITES, drained the same way: every targeting break
                // is here whatever it carries, a valueless one skipping the
                // same window (docs/impl/region/anchors.md).
                if let Some(sites) = self.block_break_nodes.remove(block_id) {
                    self.break_skip_blocks.push((hir.id, sites));
                }
                dedup_regions(&mut last);
                last
            }

            // A `break` yields no value of its own — control leaves through the
            // target block's exit label — but the value it carries becomes that
            // block's value. Record the regions against the target so the
            // `Block` arm above can union them into its result.
            HirKind::Break { block_id, value } => {
                let regions = self.walk(value);
                // Record the site itself before the region filter below: the
                // skipped-release window is a property of where control leaves,
                // which every break has, including one carrying no region at all.
                self.block_break_nodes
                    .entry(*block_id)
                    .or_default()
                    .push(hir.id);
                // Never a CALLER arg region reached inside an inline re-walk —
                // the same filter the `Return` arm applies. Outside an inline
                // `inline_bound_regions` is empty and nothing is dropped.
                let owned: Vec<Region> = regions
                    .iter()
                    .copied()
                    .filter(|r| !self.inline_bound_regions.contains(r))
                    .collect();
                if !owned.is_empty() {
                    self.block_break_regions
                        .entry(*block_id)
                        .or_default()
                        .extend(owned);
                }
                Vec::new()
            }

            HirKind::Call { .. } => self.walk_call(hir),

            HirKind::SetCell { cell, value } => {
                let _ = self.walk(cell);
                let val_regions = self.walk(value);
                if let HirKind::Var(b) = &cell.kind {
                    if let Some(&cell_binding_region) = self.binding_region.get(b) {
                        for &r in &val_regions {
                            self.record_edge(hir.id, r, cell_binding_region);
                        }
                    }
                    self.record_top_level_reassign(*b, hir.id, &val_regions);
                }
                val_regions
            }

            HirKind::DerefCell { cell } => {
                // Transparent at the lowerer: no instruction is emitted at this
                // HirId, and walking the cell-`Var` returns the regions that
                // flowed into the cell.
                self.walk(cell)
            }

            HirKind::Emit { value, .. } => {
                // No compile-time edge: the runtime incref at `handle_emit`
                // covers the operand. The payload's regions are RECORDED rather
                // than returned — an `Emit` evaluates to the resume value — so
                // the borrowed-payload pass can read them
                // (docs/impl/region/owner.md).
                let payload = self.walk(value);
                self.emit_payload_regions.insert(hir.id, payload);
                // The resume value arrives uncounted, so mirror `Eval`: a
                // placeholder call-result region balances the lowerer's
                // `LoadResumeValue` mint at this node's `decref_point`.
                let result_r = self.alloc_here(hir.id);
                self.call_result_regions.insert(result_r);
                vec![result_r]
            }

            HirKind::Eval { expr, env } => {
                let _ = self.walk(expr);
                let _ = self.walk(env);
                // `Eval` runs an inner compilation whose allocations live in
                // regions opaque to the outer, so mirror `Call`: a placeholder
                // call-result region, released by value at this node's
                // `decref_point`.
                let result_r = self.alloc_here(hir.id);
                self.call_result_regions.insert(result_r);
                vec![result_r]
            }

            HirKind::Assign { target, value } => {
                let val_regions = self.walk(value);
                if let Some(&target_binding_region) = self.binding_region.get(target) {
                    for &r in &val_regions {
                        self.record_edge(hir.id, r, target_binding_region);
                    }
                }
                // Update target binding's possible source regions.
                self.binding_regions
                    .entry(*target)
                    .and_modify(|v| {
                        v.extend(val_regions.iter().copied());
                        dedup_regions(v);
                    })
                    .or_insert_with(|| val_regions.clone());
                self.record_top_level_reassign(*target, hir.id, &val_regions);
                val_regions
            }

            HirKind::Define { binding, value } => {
                let val_regions = self.walk(value);
                self.binding_region.insert(*binding, self.current_region);
                // Union, never overwrite: sibling top-level `(def x …)` forms
                // share one `Binding`, and an earlier `Begin` pre-pass cell
                // region has to survive this walk.
                let entry = self.binding_regions.entry(*binding).or_default();
                for r in val_regions.iter().copied() {
                    if !entry.contains(&r) {
                        entry.push(r);
                    }
                }
                self.record_binder_init_site(*binding, value.id);
                // A captured local materialized as a `populate_env` env cell
                // takes a cell placeholder in ADDITION to its init regions,
                // never in place of them: the two releases are independent
                // (docs/impl/region/cells.md). A captured PARAM has no init
                // region to preserve and does replace.
                if let Some(cell_r) = self.env_cell_placeholder(*binding) {
                    let entry = self.binding_regions.entry(*binding).or_default();
                    if !entry.contains(&cell_r) {
                        entry.push(cell_r);
                    }
                }
                val_regions
            }

            HirKind::Destructure { pattern, value, .. } => {
                let val_regions = self.walk(value);
                // A `Destructure` CONSUMES its value: the field extraction reads
                // it after the value expression's own last read, so the
                // post-pass extends the value's regions to this node
                // (docs/impl/region/rules.md Rule 4).
                self.destructure_sites.push((hir.id, val_regions.clone()));
                self.record_destructure_rest_regions(hir.id, pattern);
                for b in pattern.bindings().bindings {
                    self.binding_region.insert(b, self.current_region);
                    // A leaf NAMES the source without holding it, which the
                    // region set below cannot express on its own
                    // (docs/impl/region/relocate.md).
                    self.destructure_leaf_bindings.insert(b);
                    // A destructured binding may hold a value living in the
                    // source's regions, so propagate them conservatively. Union,
                    // never overwrite: one arena `Binding` can be assigned by
                    // several destructures in a file, and overwriting drops an
                    // earlier source
                    // (`letrec_init_does_not_overwrite_destructure_binding_regions`).
                    let entry = self.binding_regions.entry(b).or_default();
                    for r in val_regions.iter().copied() {
                        if !entry.contains(&r) {
                            entry.push(r);
                        }
                    }
                }
                val_regions
            }

            HirKind::Parameterize { bindings, body } => {
                for (k, v) in bindings {
                    let _ = self.walk(k);
                    let _ = self.walk(v);
                }
                self.walk(body)
            }

            HirKind::While { cond, body } => {
                let may_suspend = hir.signal.may_suspend();
                if !may_suspend {
                    let r = self.fresh_region(self.current_region);
                    self.scope_region.insert(hir.id, r);
                    let saved = self.current_region;
                    self.current_region = r;
                    let _ = self.walk(cond);
                    let _ = self.walk(body);
                    self.current_region = saved;
                } else {
                    let _ = self.walk(cond);
                    let _ = self.walk(body);
                }
                Vec::new()
            }

            HirKind::Intrinsic { .. } => self.walk_intrinsic(hir),

            HirKind::Return { value } => {
                // Region-transparent: the result is the same value in the same
                // regions, so no alloc and no edge. The node is recorded for the
                // post-pass, which orders each region's release after the
                // `IncrefValueRegion` the lowerer emits here.
                let regions = self.walk(value);
                // Never a CALLER arg region reached inside an inline re-walk:
                // the caller owns that region's release. Outside an inline
                // `inline_bound_regions` is empty and nothing is dropped.
                let owned: Vec<Region> = regions
                    .iter()
                    .copied()
                    .filter(|r| !self.inline_bound_regions.contains(r))
                    .collect();
                if !owned.is_empty() {
                    self.return_sites.push((hir.id, owned));
                }
                regions
            }

            HirKind::Error => Vec::new(),
            _ => unreachable!("walk_rest: HIR kind handled in walk"),
        }
    }
}
