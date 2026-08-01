//! Region walk for the remaining HIR forms (Match through Error),
//! split out of `RegionInference::walk` to keep that dispatcher small.

use super::*;

impl RegionInference {
    pub(super) fn walk_rest(&mut self, hir: &Hir) -> Vec<Region> {
        match &hir.kind {
            HirKind::Match { value, arms } => {
                // Register the Match node for pattern-level allocations
                // (ArrayMutSliceFrom, StructRest from destructuring).
                self.alloc_here(hir.id);
                let val_regions = self.walk(value);
                let mut out = Vec::new();
                for (pat, guard, body) in arms {
                    for b in pat.bindings().bindings {
                        self.binding_region.insert(b, self.current_region);
                        // A pattern binding may ALIAS into the scrutinee's
                        // region(s): `(a & rest)` binds `rest` to a sublist
                        // sharing the subject's cells, `(h . t)` / an
                        // immutable-array element `[a b]` / an immutable-struct
                        // value `{:k v}` likewise hand back a pointer co-located
                        // in the subject's region pages (docs/impl/region/model.md
                        // § "RegionSlice contents share their object's region").
                        // Conservatively propagate the scrutinee's regions —
                        // EXACTLY as the `HirKind::Destructure` arm does for
                        // `(def (a & r) …)` — so uses of the bound alias extend
                        // the subject region's `decref_point` (liveness.rs) and
                        // emit the right cross-region increfs on escape. Without
                        // this the subject region is freed at the match's own
                        // `decref_point` while a bound sublist/element still
                        // points into it (region-match-rest-uaf.lisp; the
                        // advanced.lisp guard-with-rest UAF). Union, never
                        // overwrite: the same arena Binding id can be targeted by
                        // multiple matches/destructures, and overwriting drops an
                        // earlier source region (see the `Destructure` arm).
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
                // Register Begin for pre-allocated capture cells
                // (MakeCaptureCell in lower_begin for Define bindings
                // with needs_capture).
                //
                // Predicate mirrors lower_begin: emit MakeCaptureCell iff
                // (a) we are NOT inside a lambda body (the VM materializes
                // cells via the closure-construction path inside lambdas),
                // AND (b) at least one reachable Define/Destructure binding
                // has `needs_capture()` true (reachable via Let/Begin/Loop/
                // Block, NOT via If/Match/Cond/Lambda — see
                // `collect_preallocate_bindings`). Unconditional alloc_here
                // here would create phantom regions whose DecrefRegion is
                // emitted at the Begin's decref_point but never paired with a
                // runtime alloc_in_region.
                if !self.in_lambda() && self.begin_has_capturable_binding(exprs) {
                    // ONE region PER pre-allocated capture cell, never one
                    // region for all of them: the runtime mints a fresh
                    // physical region per allocation *execution* and
                    // overwrites the slot's activation mapping, so N
                    // MakeCaptureCells against one slot orphan the first
                    // N−1 regions — the shared-slot capture-cell leak
                    // (docs/impl/region/model.md, "one allocation execution per slot
                    // between drops"; region-capture-cell-shared-slot-leak.lisp).
                    //
                    // Each cell must outlive ITS binding's last use —
                    // including uses in sibling top-level forms (the
                    // file-letrec lifts every top-level form into a sibling
                    // init, and bindings introduced inside one form's Begin
                    // can still be referenced by a later sibling). Recording
                    // the cell's region in `binding_regions[b]` lets the
                    // post-pass `decref_point` extension (compute_last_use
                    // over the binding's uses) cover it — without this, the
                    // cell is freed at the Begin's own decref_point and the
                    // next access reads a dangling CaptureCell (the tag=0x22
                    // TAG_CAPTURE_CELL UAF at `as_capture_cell` in
                    // handle_update_capture).
                    let mut capturable = Vec::new();
                    Self::collect_begin_capturable_bindings(self.arena(), exprs, &mut capturable);
                    for b in capturable {
                        let cell_region = self.fresh_region(self.current_region);
                        self.begin_cell_regions
                            .entry(hir.id)
                            .or_default()
                            .push((b, cell_region));
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
                // (docs/impl/region/mechanism.md § "`break` transfers its value;
                // it does not consume it"). This is what lets a binding named to
                // the block's value hold the broken value's regions, so the
                // binding-chain `decref_point` extension carries the release
                // past that binding's own reads instead of leaving it at the
                // block's exit label. Taken, not read: the entry belongs to this
                // block alone, and clearing it keeps an outer block from
                // inheriting an inner block's breaks.
                //
                // The same regions are recorded as a `break_sites` entry against
                // THIS node, the transferring-node dual of `return_sites`: the
                // post-pass pins each one's `decref_point` to where this block's
                // value is consumed, because a release left inside the body is
                // jumped over by the break and never runs.
                if let Some(broken) = self.block_break_regions.remove(block_id) {
                    self.break_sites.push((hir.id, broken.clone()));
                    last.extend(broken);
                }
                // The break SITES, drained the same way and for the same reason
                // the region entry is: the window a break jumps over belongs to
                // this block alone. Every targeting break is here, whatever it
                // carries — a `(break 1)` carries no region yet still skips every
                // release from its own node to the exit label
                // (docs/impl/region/mechanism.md § "A release the break jumps over
                // is not a release").
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
                // the same filter the `Return` arm applies, for the same reason:
                // `try_inline_call` binds the inlined callee's params to the
                // caller's arg regions, so a `break` here can name a caller
                // region whose release the caller owns. Outside an inline
                // `inline_bound_regions` is empty, so the structural walk
                // records every break unchanged.
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
                // Transparent at the lowerer: `lower_deref_cell` delegates
                // to `lower_expr(cell)` and `lower_var` auto-unwraps the
                // CaptureCell via `LoadCapture`. No instruction is emitted
                // at this HirId. The loaded value lives in whichever
                // region(s) flowed into the cell via its initial value
                // or SetCell — walking the cell-Var returns exactly that
                // set via `binding_regions[b]`.
                self.walk(cell)
            }

            HirKind::Emit { value, .. } => {
                // No compile-time edge: the runtime incref at handle_emit (step 14)
                // keeps the operand region alive past the matching DecrefRegion at
                // the resume site. The fiber-frontier escape of the emitted value is
                // escape's judgment (`analyze_escape`'s fiber/emit facet, projected
                // by `region::infer::escape`), not a solver-recorded region set — so the
                // walk here just records the operand's edges / binding flow.
                let _ = self.walk(value);
                Vec::new()
            }

            HirKind::Eval { expr, env } => {
                let _ = self.walk(expr);
                let _ = self.walk(env);
                // Eval runs an inner compilation whose allocations live
                // in regions opaque to the outer. Mirror Call: allocate
                // a placeholder region and register it as a Call-result
                // so the lowerer emits `DecrefValueRegion(expected)`
                // (value-gated) at the Eval's decref_point. The runtime
                // skips the decref when `region_of(value)` doesn't
                // match the placeholder — safe by construction here.
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
                // Union with any existing entry instead of overwriting:
                // the same Binding id can be re-assigned by sibling
                // top-level (def …) forms (the file-letrec lifts each
                // top-level form into its own init, so two forms that
                // both `(def x …)` share the same Binding id), and an
                // earlier Begin-pre-pass insertion of a CaptureCell
                // region must be preserved across the later Define's
                // walk. Same overwrite-vs-union class as Destructure
                // (commit 204e5ebb) and Letrec.
                let entry = self.binding_regions.entry(*binding).or_default();
                for r in val_regions.iter().copied() {
                    if !entry.contains(&r) {
                        entry.push(r);
                    }
                }
                // Captured local materialized as a `populate_env` env cell:
                // ADD a cell placeholder so the lowerer releases the CELL at the
                // binding's last use (`DecrefCellRegion`), WITHOUT dropping the
                // init value's own region(s). Crucially we must NOT clobber
                // `binding_regions[binding]` to `[cell_r]`: a call-result init
                // (e.g. `(def @f (fiber/new …))`) has only a placeholder region
                // whose `decref_point` is driven *solely* by this binding's
                // entry — dropping it frees the call result at the Define while
                // the binding is still live (a `DecrefValueRegion of fiber` UAF,
                // witnessed by table-keys.lisp). The init region's release and
                // the cell's release are independent: the `StoreCapture` increfs
                // the inner value's region, the init's own decref drops one ref,
                // and the cell's free-time cascade drops the other — balanced.
                // (Captured *params* clobber to `[cell_r]` because a param has no
                // init region to preserve.)
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
                // A Destructure CONSUMES its value: the field extraction
                // reads it after the value expression's last read. Record
                // the site so the post-pass extends the value's regions'
                // decref_point to this node (docs/impl/region/rules.md Rule 4) —
                // otherwise an all-bindings-unused destructure (the
                // `&named`-param prologue) frees the source before the
                // extraction (region-named-param-uaf.lisp).
                self.destructure_sites.push((hir.id, val_regions.clone()));
                for b in pattern.bindings().bindings {
                    self.binding_region.insert(b, self.current_region);
                    // Destructured bindings may hold values that live in
                    // the source's region(s) — `(rest list)` returns a
                    // sublist sharing list's region; `(first xs)` on a
                    // list of heap values returns an element in xs's
                    // region. Conservatively propagate the source's
                    // regions so uses of the destructured binding emit
                    // the right cross-region increfs (and extend
                    // decref_point through binding uses; see liveness.rs).
                    //
                    // Union with any existing entry instead of
                    // overwriting: the same pattern Binding id can be
                    // assigned by multiple destructures in the same
                    // file (top-level re-defs like the destructure.lisp
                    // tests rebind `r` across many `(def (… & r) …)`
                    // forms — sometimes at the file's letrec level,
                    // sometimes nested inside begins; both share the
                    // arena's `r` Binding id). Overwriting drops the
                    // earlier source region and the post-pass
                    // decref_point extension never reaches the earlier
                    // list, leaving it freed before its in-begin
                    // consumer reads `r` (counter-factual:
                    // `letrec_init_does_not_overwrite_destructure_
                    // binding_regions`).
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
                // Region-transparent: the result is the same value in
                // the same region(s). Record the node so the post-pass
                // extends those regions' `decref_point` to here, ordering
                // their DecrefRegion after the `IncrefValueRegion` the
                // lowerer emits for this node. No alloc, no edge.
                let regions = self.walk(value);
                // Record the returned regions for the post-pass `decref_point`
                // extension — but never a CALLER arg region reached inside an
                // inline re-walk. `try_inline_call` binds the inlined callee's
                // params to the caller's arg regions, so a `Return` here can name
                // a caller region; extending its `decref_point` to this callee
                // node is wrong (the caller owns that region's release). For a
                // self-tail-recursive callee whose accumulator arg the tail call
                // transfers forward — stdlib `fold`'s `go` threading the reducer
                // result — pinning the arg to the base-case (sibling) arm makes
                // the branch-union release over-free it under self-tail-call frame
                // reuse. Outside an inline `inline_bound_regions` is empty, so the
                // structural walk records every return unchanged; the callee's own
                // structural walk still records its genuine body-result returns
                // (those are not arg regions), so the call site loses nothing.
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
