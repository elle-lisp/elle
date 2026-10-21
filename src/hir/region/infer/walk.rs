// audited: 2026-10-21
//! The region walk's dispatch: the source regions each HIR node yields, and its literal, lambda, scope and branch arms.
//!
//! docs/impl/region/model.md
//! docs/impl/region/rules.md

use super::*;

impl RegionInference {
    /// Walk the HIR tree. Returns the set of source regions a value
    /// produced by this expression may belong to. For multi-branch
    /// expressions (If, Cond, Match, And, Or) the result is the union
    /// of branches' sets; downstream edges are emitted against every
    /// possible source. Empty vec means "no heap value" (immediate,
    /// nil, constant-pool string/quote).
    pub(super) fn walk(&mut self, hir: &Hir) -> Vec<Region> {
        match &hir.kind {
            // Immediates and interned constants: no heap allocation, no region.
            // A keyword is a tag+hash immediate. `Quote` holds an immediate or,
            // on the macro-hygiene path, a pre-baked pool Value that is its own
            // root, so it has no per-activation region either.
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_) => Vec::new(),

            // A string literal AND quoted compound data are ordinary heap
            // allocations, each in its OWN region (the one-region-per-value
            // baseline). `MaterializeConst` builds each fresh into that region
            // every execution, and it is freed at its `decref_point` like any
            // value. Its region flows up so the caller tracks every escape
            // (return/store/capture/call-arg) by normal RC
            // (docs/impl/region/model.md). A quoted aggregate's whole structure
            // shares this one region.
            HirKind::String(_) | HirKind::QuoteConst(_) => {
                let r = self.alloc_here(hir.id);
                vec![r]
            }

            HirKind::MakeCell { value } => {
                // Transparent at the lowerer: `lower_make_cell` delegates
                // to `lower_expr(value)` and the actual cell allocation
                // happens implicitly in `lower_let`/`lower_letrec` at the
                // binding site. So MakeCell introduces no instruction at
                // this HirId — pass the value's regions through.
                self.walk(value)
            }

            HirKind::Lambda(lambda) => {
                let lambda_r = self.alloc_here(hir.id);

                // Captures intentionally record NO cross-region edge. A
                // captured heap value becomes scannable content of the
                // Closure env. The runtime auto-incref (`alloc_obj` →
                // `incref_cross_region_refs` → `find_object_cross_refs`, which
                // covers `Closure { env }`) already pins each captured region.
                // The cascade decref in `free_runtime_region_pages` balances it
                // when the closure region dies. A static `IncrefRegion` here
                // would be a *second* incref against that *single* cascade
                // decref: net +1 per closure, a per-iteration leak in loops.
                // Captures of immediates have no region and are a no-op. See
                // `vm/closure.rs`.

                // Body in a fresh scope region.
                let body_region = self.fresh_region(self.current_region);
                self.scope_region.insert(hir.id, body_region);
                let saved = self.current_region;
                self.current_region = body_region;

                // A non-captured fixed param is an OWNED binding (like a
                // call-result): the caller moves it one reference, and the
                // callee releases it at the param's true last use. Mint a
                // placeholder region per such param and register it in
                // `call_result_regions`, mirroring the opaque-Call treatment
                // (docs/impl/region/rules.md). The lowerer emits a
                // value-based `DecrefValueRegion` that reads the param's slot
                // at `decref_point`, releasing the arg's *runtime* region.
                //
                // We do NOT `alloc_here` for it: a param arrives on the stack
                // and has no allocation instruction of its own. So the region
                // is "phantom", absent from `live_regions`, and `build_info`
                // filters out any cross-region edge originating from it. The
                // runtime auto-incref over a stored value's region already
                // covers escape, so a compile-time `IncrefRegion(param_r)` would
                // double-count, exactly the capture-edge hazard above.
                //
                // The binding-chains post-pass sets `param_r`'s `decref_point`
                // from `p`'s uses. A param forwarded as a tail-call arg gets
                // the tail-call node (dead there, so MOVED to the callee). An
                // unused param gets no `region_data` entry, and the lowerer
                // releases it after the body (`lir::lower::lambda::body`).
                for p in &lambda.params {
                    self.binding_region.insert(*p, body_region);
                    if self.arena().get(*p).needs_capture() {
                        // Captured (env-allocated) param: `populate_env` wraps it
                        // in a CaptureCell minted in its OWN region. Give it a
                        // phantom cell placeholder, marked in
                        // `cell_release_regions`, so the lowerer releases that
                        // cell at the param's last use via `DecrefCellRegion`
                        // (region_of the cell). Phantom (no `alloc_here`) means
                        // it is filtered from `live_regions`, so it records no
                        // compile-time `IncrefRegion` edge. The runtime
                        // auto-incref over the wrapped value covers escape, and
                        // the closure-capture incref and free cascade balance
                        // the cell's cross-region edge. `decref_point` comes from
                        // the binding-chains post-pass over `p`'s uses.
                        let cell_r = self.fresh_region(body_region);
                        self.call_result_regions.insert(cell_r);
                        self.cell_release_regions.insert(cell_r);
                        self.binding_regions.insert(*p, vec![cell_r]);
                    } else {
                        let param_r = self.fresh_region(body_region);
                        self.call_result_regions.insert(param_r);
                        self.binding_regions.insert(*p, vec![param_r]);
                    }
                }
                if let Some(rp) = &lambda.rest_param {
                    self.binding_region.insert(*rp, body_region);
                    match &lambda.vararg_kind {
                        // Each collector is an OWNED value the callee's calling
                        // convention builds at run time, with rc=1 and no caller
                        // incref. `args_to_list` builds the `&` rest LIST per
                        // cons with ownership transferred to the head, so one
                        // value-based release of the head cascade-frees the
                        // list. `collect_struct_in_own_region` mints the single
                        // `&keys`/`&named` struct in its own region. So the
                        // callee releases the collector by value at the rest
                        // param's last use, like a non-captured owned param.
                        //
                        // Give it a phantom placeholder in `call_result_regions`:
                        // no `alloc_here`, because no compiled instruction
                        // allocates it. The binding-chains post-pass sets its
                        // `decref_point` from `rp`'s last use:
                        //   - `&named` (`(&named @flag) flag`): the synthetic
                        //     `__named_param`'s last use is the param-destructure
                        //     that extracts the named bindings.
                        //   - a native tail call (`(& xs) (length xs)`): the
                        //     post-`TailCall` `DecrefValueRegion` runs when the
                        //     dispatch loop continues past the native.
                        //   - a closure tail call (`(& xs) (sink xs)`): the move
                        //     transfers to the callee, which releases its owned
                        //     param.
                        crate::hir::VarargKind::List
                        | crate::hir::VarargKind::Struct
                        | crate::hir::VarargKind::StrictStruct(_) => {
                            let rest_r = self.fresh_region(body_region);
                            self.call_result_regions.insert(rest_r);
                            self.binding_regions.insert(*rp, vec![rest_r]);
                        }
                    }
                }

                self.in_lambda_depth += 1;
                self.walking_lambdas.push(hir as *const Hir);
                // Walk the body for its edges / binding flow / call-result regions.
                // The body's tail regions are not a solver fact: escape's return
                // facet (`analyze_escape`) owns that judgment, projected to regions
                // by `region::infer::escape`.
                self.walk(&lambda.body);
                self.walking_lambdas.pop();
                self.in_lambda_depth -= 1;
                self.current_region = saved;

                vec![lambda_r]
            }

            HirKind::Var(b) => self.binding_regions.get(b).cloned().unwrap_or_default(),

            HirKind::Let { bindings, body } => {
                // One region PER capture cell the lowerer will emit at this
                // Let (`lower_let` wraps each captured binding's init in a
                // MakeCaptureCell when outside a lambda). A single shared
                // region slot orphans all but the last minted physical
                // region (docs/impl/region/model.md). The `!in_lambda` gate
                // mirrors the lowerer exactly: inside a lambda a captured let
                // binding goes through StoreCapture (no compiled cell), so a
                // region here would be a phantom whose DecrefRegion never pairs
                // with an allocation.
                if !self.in_lambda() {
                    for (b, _) in bindings {
                        if self.arena().get(*b).needs_capture() {
                            let cell_region = self.fresh_region(self.current_region);
                            self.record_compiled_cell(hir.id, *b, cell_region);
                        }
                    }
                }

                let scope_region = self.fresh_region(self.current_region);
                self.scope_region.insert(hir.id, scope_region);
                let saved = self.current_region;
                self.current_region = scope_region;

                for (b, init) in bindings {
                    if matches!(init.kind, HirKind::Lambda { .. }) {
                        self.binding_lambda.insert(*b, init as *const Hir);
                    }
                    let init_regions = self.walk(init);
                    self.binding_region.insert(*b, scope_region);
                    let init_regions = self.counted_cell_read_regions(*b, init, init_regions);
                    self.binding_regions.insert(*b, init_regions);
                    self.record_binder_init_site(*b, init.id);
                    // Captured local materialized as a `populate_env` env cell: ADD a
                    // cell placeholder so the lowerer releases the CELL at the
                    // binding's last use, WITHOUT dropping the init value's own
                    // region(s). The binder that introduced the local decides
                    // nothing about the cell. Outside a lambda,
                    // `env_cell_placeholder` answers `None`, because the arm head
                    // minted a region for the compiled `MakeCaptureCell` instead.
                    // So this is the `Define` binder's treatment, for the same
                    // reason (pinned by tests/impl/region-let-capture-cell-leak.lisp).
                    if let Some(cell_r) = self.env_cell_placeholder(*b) {
                        let entry = self.binding_regions.entry(*b).or_default();
                        if !entry.contains(&cell_r) {
                            entry.push(cell_r);
                        }
                    }
                }

                // `cell ⊇ content`: the cells minted above hold their init value by an
                // uncounted compiled store the external-uniqueness scan cannot see. Record
                // the containment now that `binding_regions` is known, keyed at the same
                // `!in_lambda` gate the mint uses. (see `record_cell_content_edges`.)
                if !self.in_lambda() {
                    self.record_cell_content_edges(hir.id);
                }

                let body_regions = self.walk(body);
                self.current_region = saved;
                body_regions
            }

            HirKind::Letrec { bindings, body } => {
                // One region PER capture cell `lower_letrec` will emit — a
                // COMPILED MakeCaptureCell: every captured binding at top
                // level, plus the immutable lambda-initialized shape inside a
                // lambda (`compiled_forward_cell`, the closure-cycle merge's
                // static-slot cells). Any other in-lambda captured binding
                // keeps the env-cell route (StoreCapture), so a region here
                // would be a phantom. One region per cell, never one shared
                // slot: N cells against one slot orphan all but the last
                // minted physical region (docs/impl/region/model.md). Skipped
                // on an inline re-walk, which would mint duplicate cells. The
                // structural walk is the sole writer, as in `alloc_here`.
                if self.inline_depth == 0 {
                    for (b, init) in bindings {
                        let init_is_lambda = matches!(init.kind, HirKind::Lambda { .. });
                        if self
                            .arena()
                            .get(*b)
                            .compiled_forward_cell(init_is_lambda, self.in_lambda())
                        {
                            let cell_region = self.fresh_region(self.current_region);
                            self.record_compiled_cell(hir.id, *b, cell_region);
                        }
                    }
                }

                let scope_region = self.fresh_region(self.current_region);
                self.scope_region.insert(hir.id, scope_region);
                let saved = self.current_region;
                self.current_region = scope_region;

                // Pre-bind all names (letrec allows mutual reference).
                for (b, _) in bindings {
                    self.binding_region.insert(*b, scope_region);
                    self.binding_regions.insert(*b, Vec::new());
                }
                for (b, init) in bindings {
                    if matches!(init.kind, HirKind::Lambda { .. }) {
                        self.binding_lambda.insert(*b, init as *const Hir);
                    }
                    let init_regions = self.walk(init);
                    // The Rule 5 counted reader, as the Let arm applies it. A
                    // letrec binding whose init is a whole-value read of a
                    // RE-STORABLE capture cell must NOT inherit the cell's static
                    // source regions. The file-letrec statement wrapper around a
                    // trailing `(deref-cell x)` read of a mutated `(var x …)` is
                    // that shape. Those regions name the init value or the cell,
                    // not whatever the cell holds at read time. A static route
                    // against them, such as a coalesced return retain through the
                    // wrapper, resolves the slot against repointed content
                    // (pinned by
                    // `file_scope::captures::test_mutable_var_mutation_visible_after_call`).
                    // The counted read mints a placeholder (a call-result
                    // region), so the reader stays value-resolved and takes its
                    // own balanced reference.
                    let init_regions = self.counted_cell_read_regions(*b, init, init_regions);
                    // Union with any existing entry instead of overwriting. An
                    // earlier init's walk may have written `binding_regions[b]`
                    // through a destructure whose pattern targets a name this
                    // letrec binds or pre-binds. The file-scope shape
                    // `[__file_expr_0 (begin (destructure (a & r) ...) ...) a nil
                    // b nil r nil ...]` from `compile_file_to_fhir` is the
                    // canonical case. Overwriting drops the destructure's
                    // contribution. The post-pass then leaves the source region's
                    // `decref_point` at the destructure, which frees it before
                    // `r`'s use reads through it (pinned by
                    // `letrec_init_does_not_overwrite_destructure_binding_regions`).
                    let entry = self.binding_regions.entry(*b).or_default();
                    for r in init_regions {
                        if !entry.contains(&r) {
                            entry.push(r);
                        }
                    }
                    self.record_binder_init_site(*b, init.id);
                }

                // `cell ⊇ content`: a compiled forward cell holds its closure (or other
                // init value) by an uncounted `StoreCaptureCell` the scan cannot see.
                // Record the containment, gated by the `inline_depth == 0` structural walk
                // that minted the cells (a re-walk at a deeper inline depth would duplicate
                // it). (see `record_cell_content_edges`.)
                if self.inline_depth == 0 {
                    self.record_cell_content_edges(hir.id);
                }

                let body_regions = self.walk(body);
                self.current_region = saved;
                body_regions
            }

            HirKind::Loop { bindings, body } => {
                let loop_region = self.fresh_region(self.current_region);
                self.scope_region.insert(hir.id, loop_region);

                // Inits evaluated in the enclosing region.
                for (b, init) in bindings {
                    let init_regions = self.walk(init);
                    self.binding_region.insert(*b, loop_region);
                    self.binding_regions.insert(*b, init_regions);
                    // A bare `Var` init hands the parameter the very reference the
                    // named binding held — functionalization's split of one source
                    // name, whose two versions the reassign gate must not count as
                    // two holders (see `loop_forwarded_params`). Recorded here
                    // because this is the line that copies one binding's source
                    // regions onto the other's.
                    if let HirKind::Var(src) = &init.kind {
                        self.loop_forwarded_params.insert(*b, *src);
                    }
                }

                let saved = self.current_region;
                self.current_region = loop_region;
                let body_regions = self.walk(body);
                self.current_region = saved;
                body_regions
            }

            HirKind::Recur { args } => {
                for a in args {
                    let _ = self.walk(a);
                }
                Vec::new()
            }

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let _ = self.walk(cond);
                let mut out = self.walk(then_branch);
                out.extend(self.walk(else_branch));
                dedup_regions(&mut out);
                out
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                let mut out = Vec::new();
                for (c, b) in clauses {
                    let _ = self.walk(c);
                    out.extend(self.walk(b));
                }
                if let Some(eb) = else_branch {
                    out.extend(self.walk(eb));
                }
                dedup_regions(&mut out);
                out
            }

            _ => self.walk_rest(hir),
        }
    }
}

/// Sort and dedup a vector of regions in place. Stable order keeps
/// the output of region inference deterministic across walks.
fn dedup_regions(v: &mut Vec<Region>) {
    v.sort_by_key(|r| r.0);
    v.dedup();
}

mod call;
mod cells;
mod classify;
mod funnel;
mod inline;
mod intrinsic;
mod tail;
mod walkrest;
