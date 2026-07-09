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
            // Immediates / interned constants — no heap allocation, no region.
            // (Keyword/Symbol are tag+hash immediates; `Quote` now holds only an
            // immediate, or — on the macro-hygiene path — a pre-baked pool Value
            // that is its own root, so still no per-activation region here.)
            HirKind::Nil
            | HirKind::EmptyList
            | HirKind::Bool(_)
            | HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::Keyword(_)
            | HirKind::Quote(_) => Vec::new(),

            // A string literal AND quoted compound data are ordinary heap
            // allocations: each gets its OWN region (the one-region-per-value
            // baseline), is materialized fresh into that region by
            // `MaterializeConst` each execution, and is freed at its
            // `decref_point` like any value. Its region flows up so the caller
            // tracks every escape (return/store/capture/call-arg) by normal RC
            // (docs/impl/region/model.md § "Constants lower as ordinary allocations"). A
            // quoted aggregate's whole structure shares this one region.
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

            HirKind::Lambda {
                params,
                rest_param,
                vararg_kind,
                body,
                ..
            } => {
                let lambda_r = self.alloc_here(hir.id);

                // Captures intentionally record NO cross-region edge. A
                // captured heap value becomes scannable content of the
                // Closure env, so the runtime auto-incref
                // (`alloc_obj` → `incref_cross_region_refs` →
                // `find_object_cross_refs`, which covers `Closure { env }`)
                // already pins each captured region, balanced by the
                // cascade decref in `free_runtime_region_pages` when the closure region
                // dies. A static `IncrefRegion` here would be a *second*
                // incref against that *single* cascade decref — net +1
                // per closure, a per-iteration leak in loops (the captured
                // region never reaches RC 0). Captures of immediates have
                // no region and are a no-op. See `vm/closure.rs`.

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
                // (docs/impl/region/rules.md Rule 2 "opaque Call"): the lowerer will emit
                // a value-based `DecrefValueRegion` (reading the param's slot)
                // at `decref_point`, releasing the arg's *runtime* region —
                // whatever it turns out to be. Crucially we do NOT `alloc_here`
                // for it: a param has no allocation instruction of its own (it
                // arrives on the stack), so the region is "phantom" — not in
                // `live_regions`, so `build_info` filters out any cross-region
                // edge originating from it (the runtime auto-incref over a
                // stored value's region already covers escape; a compile-time
                // `IncrefRegion(param_r)` would double-count, exactly the
                // capture-edge hazard at the Lambda head).
                //
                // `decref_point` for `param_r` is set by the binding-chains
                // post-pass from `p`'s uses: a param forwarded as a tail-call
                // arg gets `decref_point` = the tail-call node (dead → MOVED to
                // the callee); an unused param gets no `region_data` entry and
                // the lowerer releases it at the function's `Return`.
                //
                // LBox/captured params (`needs_capture`) live in the env and
                // are accessed via LoadCapture, not an owned local slot, so they
                // stay borrows (empty regions) — the env owns them.
                for p in params {
                    self.binding_region.insert(*p, body_region);
                    if self.arena().get(*p).needs_capture() {
                        // Captured (env-allocated) param: `populate_env` wraps it
                        // in a CaptureCell minted in its OWN region (Inc1). Give
                        // it a phantom cell placeholder so the lowerer releases
                        // that cell at the param's last use via `DecrefCellRegion`
                        // (region_of the cell), marked in `cell_release_regions`.
                        // Phantom (no `alloc_here`) → filtered from `live_regions`,
                        // so no spurious compile-time `IncrefRegion` edge (the
                        // runtime auto-incref over the wrapped value covers escape;
                        // the closure-capture incref + free-cascade balance the
                        // cell's cross-region edge). `decref_point` comes from the
                        // binding-chains post-pass over `p`'s uses (the capture).
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
                if let Some(rp) = rest_param {
                    self.binding_region.insert(*rp, body_region);
                    match vararg_kind {
                        // `&keys`/`&named` collect their keyword args into a
                        // SINGLE struct, minted in its OWN region by
                        // `collect_struct_in_own_region` (Inc1). That struct is
                        // an OWNED value (rc=1, no caller incref — built by the
                        // callee's calling convention), so the callee must
                        // release it value-based at its last use, exactly like a
                        // non-captured owned param. Give it a phantom placeholder
                        // in `call_result_regions` (no `alloc_here`: the struct's
                        // alloc is the runtime `collect_struct_in_own_region`, not
                        // a compiled instruction). The binding-chains post-pass
                        // sets its `decref_point` from `rp`'s last use:
                        //   - `&named` (`(&named @flag) flag`): the synthetic
                        //     `__named_param`'s last use is the param-destructure
                        //     that extracts the named bindings → released there
                        //     (this increment).
                        //   - `&keys opts` whose body tail-calls a native
                        //     (`(length opts)`): last use is the native tail
                        //     call → dead past `TailCall` → released by the
                        //     native-tail path (Increment 4).
                        // docs/impl/region/rules.md Rule 8 (the unmodeled env region).
                        // `&keys`/`&named` collect into a single struct; the
                        // variadic rest LIST is built per-cons with ownership
                        // transferred to the HEAD (`args_to_list`), so a single
                        // value-based release of the head cascade-frees the whole
                        // list. All three are OWNED rest collections the callee
                        // releases value-based at the rest-param's last use — give
                        // each an owned placeholder. For a native tail call
                        // (`(& xs) (length xs)`), the release rides the
                        // not-frame-replacing native-tail path (Inc4): the
                        // compiler's post-`TailCall` `DecrefValueRegion` runs when
                        // the dispatch loop continues past the native. For a
                        // closure tail call (`(& xs) (sink xs)`) the move transfers
                        // to the callee, which releases its owned param.
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
                // Walk the body for its edges / binding flow / call-result regions.
                // The body's tail regions — the return-as-escape frontier — are no
                // longer a solver fact: escape (`analyze_escape`'s return facet) owns
                // that judgment, projected to regions by `regions::escape`.
                self.walk(body);
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
                // region — the shared-slot capture-cell leak (docs/impl/region/model.md,
                // "one allocation execution per slot between drops"). The
                // `!in_lambda` gate mirrors the lowerer exactly: inside a
                // lambda a captured let binding goes through StoreCapture
                // (no compiled cell), so a region here would be a phantom
                // whose DecrefRegion never pairs with an allocation.
                if !self.in_lambda() {
                    for (b, _) in bindings {
                        if self.arena().get(*b).needs_capture() {
                            let cell_region = self.fresh_region(self.current_region);
                            self.begin_cell_regions
                                .entry(hir.id)
                                .or_default()
                                .push((*b, cell_region));
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
                // lambda (`letrec_compiled_cell`, the closure-cycle merge's
                // static-slot cells). Any other in-lambda captured binding
                // keeps the env-cell route (StoreCapture), so a region here
                // would be a phantom. One region per cell, never one shared
                // slot — N cells against one slot orphan all but the last
                // minted physical region (docs/impl/region/model.md, "one
                // allocation execution per slot between drops"). Skipped on an
                // inline re-walk: `try_inline_call` revisits a callee's body
                // with the CALLER's lambda depth, which would mint duplicate
                // (and wrongly-classified) cells — the structural walk is the
                // sole writer, mirroring `alloc_here`'s re-walk idempotency.
                if self.inline_depth == 0 {
                    for (b, init) in bindings {
                        let init_is_lambda = matches!(init.kind, HirKind::Lambda { .. });
                        if self
                            .arena()
                            .get(*b)
                            .letrec_compiled_cell(init_is_lambda, self.in_lambda())
                        {
                            let cell_region = self.fresh_region(self.current_region);
                            self.begin_cell_regions
                                .entry(hir.id)
                                .or_default()
                                .push((*b, cell_region));
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
                    // Rule 5 counted reader, exactly as the Let arm applies it:
                    // a letrec binding whose init is a whole-value read of a
                    // RE-STORABLE capture cell (the file-letrec statement
                    // wrapper around a trailing `(deref-cell x)` read of a
                    // mutated `(var x …)`) must NOT inherit the cell's static
                    // source regions — those name the init value (or the cell),
                    // not whatever the cell holds at read time, and a static
                    // route against them (a coalesced return retain through the
                    // wrapper) resolves the slot against repointed content (the
                    // `AssertRegionMatches` mis-coalesce;
                    // file_scope::captures::test_mutable_var_mutation_visible_after_call).
                    // The counted read mints a placeholder (a call-result
                    // region), so the reader stays value-resolved and takes its
                    // own balanced reference.
                    let init_regions = self.counted_cell_read_regions(*b, init, init_regions);
                    // Union with any existing entry instead of
                    // overwriting. An earlier init's walk may have
                    // side-effected `binding_regions[b]` via a
                    // destructure whose pattern targets a name later
                    // bound (or pre-bound) in this same letrec — the
                    // file-scope shape `[__file_expr_0 (begin
                    // (destructure (a & r) ...) ...) a nil b nil
                    // r nil ...]` emitted by compile_file_to_fhir is
                    // the canonical case. Overwriting drops the
                    // destructure's contribution and the post-pass
                    // skip-empty short-circuit then leaves the source
                    // region's decref_point at the destructure id,
                    // letting it be freed before `r`'s use reads
                    // through to a stale ptr (counter-factual:
                    // `letrec_init_does_not_overwrite_destructure_
                    // binding_regions`).
                    let entry = self.binding_regions.entry(*b).or_default();
                    for r in init_regions {
                        if !entry.contains(&r) {
                            entry.push(r);
                        }
                    }
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
mod inline;
mod intrinsic;
mod tail;
mod walkrest;
