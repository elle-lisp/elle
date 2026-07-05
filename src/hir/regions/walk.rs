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
            // (docs/impl/region-model.md § "Constants lower as ordinary allocations"). A
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
                // (docs/impl/region-rules.md Rule 2 "opaque Call"): the lowerer will emit
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
                        // docs/impl/region-rules.md Rule 8 (the unmodeled env region).
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
                // region — the shared-slot capture-cell leak (docs/impl/region-model.md,
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
                // minted physical region (docs/impl/region-model.md, "one
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
    /// Try to inline a Call's callee Lambda body for region analysis.
    ///
    /// When the callee is a Var whose binding has a known Lambda init
    /// (recorded in `binding_lambda`), temporarily bind the Lambda's
    /// params to the caller's arg source regions and walk the body.
    /// This lets the walk see intrinsics inside the body (e.g.
    /// `%array-push` inside `push`) and emit the corresponding
    /// cross-region edges at the call site.
    ///
    /// Returns `Some(result_regions)` when inlining succeeded;
    /// `None` to fall back to opaque-call handling.
    fn try_inline_call(
        &mut self,
        func: &Hir,
        arg_regions: &[Vec<Region>],
        _call_id: HirId,
    ) -> Option<Vec<Region>> {
        // Only inline Var callees.
        let binding = match &func.kind {
            HirKind::Var(b) => *b,
            _ => return None,
        };
        // Must be immutable and have a known Lambda body.
        let bi = self.arena().get(binding);
        if !bi.is_immutable || bi.is_mutated {
            return None;
        }
        let lambda_ptr = *self.binding_lambda.get(&binding)?;
        // Guard against infinite recursion (max 4 levels).
        if self.inline_depth >= 4 {
            return None;
        }
        // SAFETY: lambda_ptr points into the HIR tree which outlives
        // the RegionInference (both live for the analyze_regions call).
        let lambda = unsafe { &*lambda_ptr };
        let (params, rest_param, body) = match &lambda.kind {
            HirKind::Lambda {
                params,
                rest_param,
                body,
                ..
            } => (params, rest_param, body),
            _ => return None,
        };
        // Save and bind params to caller's arg regions.
        let mut saved: Vec<(Binding, Option<Vec<Region>>)> = Vec::new();
        for (i, p) in params.iter().enumerate() {
            saved.push((*p, self.binding_regions.get(p).cloned()));
            let regions = arg_regions.get(i).cloned().unwrap_or_default();
            self.binding_regions.insert(*p, regions);
            self.binding_region.insert(*p, self.current_region);
        }
        if let Some(rp) = rest_param {
            saved.push((*rp, self.binding_regions.get(rp).cloned()));
            self.binding_regions.insert(*rp, Vec::new());
            self.binding_region.insert(*rp, self.current_region);
        }
        self.inline_depth += 1;
        let result = self.walk(body);
        self.inline_depth -= 1;
        // Restore saved param region sets.
        for (p, prev) in saved {
            match prev {
                Some(v) => {
                    self.binding_regions.insert(p, v);
                }
                None => {
                    self.binding_regions.remove(&p);
                }
            }
        }
        Some(result)
    }

    /// Check if a call's callee is known to return an immediate value
    /// (no heap allocation). Uses the call classification data.
    fn call_returns_immediate(&self, func: &Hir) -> bool {
        if let HirKind::Var(binding) = &func.kind {
            // Check user_immediates first (letrec-bound lambdas)
            if self.call_class.user_immediates.contains(binding) {
                return true;
            }
            let bi = self.arena().get(*binding);
            // Only trust immutable bindings (primitives, not user-shadowed)
            if !bi.is_immutable || bi.is_mutated {
                return false;
            }
            self.call_class.intrinsic_ops.contains(&bi.name)
                || self.call_class.effects.get(&bi.name)
                    == Some(&crate::primitives::def::RegionEffect::Immediate)
        } else {
            false
        }
    }

    /// The callee's declared `RegionEffect` (docs/impl/region-effects.md "Native
    /// region effects"), when the callee is an immutable, unshadowed
    /// binding naming a declared primitive. `None` for unknown callees —
    /// the caller must treat that as `Mixed` (the full arg clique).
    pub(super) fn call_effect(&self, func: &Hir) -> Option<crate::primitives::def::RegionEffect> {
        if let HirKind::Var(binding) = &func.kind {
            let bi = self.arena().get(*binding);
            if !bi.is_immutable || bi.is_mutated {
                return None;
            }
            self.call_class.effects.get(&bi.name).copied()
        } else {
            None
        }
    }

    /// Is the callee a value-RETAINING store funnel (`%put`/`%array-push`/`%add`)
    /// — a `Funnel` op whose runtime body increfs the stored value? Under the same
    /// unshadowed-immutable-primitive condition as [`call_effect`]. Distinguishes
    /// the store funnels from the removals (`%del`) and byte-copy pushes
    /// (`%string-push`/`%bytes-push`), all of which share the `Funnel` effect.
    pub(super) fn is_retaining_store(&self, func: &Hir) -> bool {
        if let HirKind::Var(binding) = &func.kind {
            let bi = self.arena().get(*binding);
            if !bi.is_immutable || bi.is_mutated {
                return false;
            }
            self.call_class.retaining_store_funnels.contains(&bi.name)
        } else {
            false
        }
    }

    /// The callee's declared [`RetType`](crate::primitives::def::RetType), under
    /// the same unshadowed-immutable-primitive condition as [`call_effect`].
    /// `None` for an unknown/shadowed callee or an empty classification. The
    /// ownership inference uses this to recognize a `Funnel` store's container
    /// argument as a mutable *retaining* container (`MutableArray`/`MutableStruct`).
    pub(super) fn call_rettype(&self, func: &Hir) -> Option<crate::primitives::def::RetType> {
        if let HirKind::Var(binding) = &func.kind {
            let bi = self.arena().get(*binding);
            if !bi.is_immutable || bi.is_mutated {
                return None;
            }
            self.call_class.ret_types.get(&bi.name).copied()
        } else {
            None
        }
    }

    /// The 0-based argument indices the callee EMBEDS into its fresh result
    /// ([`crate::primitives::def::PrimitiveDef::embeds`]), under the same
    /// unshadowed-immutable-primitive condition as [`call_effect`]. Empty for an
    /// unknown/shadowed callee or an empty classification. The walk's `Fresh` arm
    /// records a `result ⊇ arg` containment edge for each, so the ownership forest
    /// tracks an argument the fresh result keeps a reference to (`with-traits`'s trait
    /// table into its cloned result). The returned slice is `'static` (the primitive
    /// table's own data), so it borrows nothing from `self`.
    pub(super) fn call_embeds(&self, func: &Hir) -> &'static [usize] {
        if let HirKind::Var(binding) = &func.kind {
            let bi = self.arena().get(*binding);
            if !bi.is_immutable || bi.is_mutated {
                return &[];
            }
            self.call_class.embeds.get(&bi.name).copied().unwrap_or(&[])
        } else {
            &[]
        }
    }

    /// Check if a HIR body is a tail call (or control flow where all result
    /// positions are tail calls). When the body is a tail call, RegionExit
    /// fires BEFORE the tail call executes, so the tail call's result does
    /// not flow through the scope — skip the body escape constraint.
    fn _is_tail_call_body(hir: &Hir) -> bool {
        match &hir.kind {
            HirKind::Call { is_tail: true, .. } => true,
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => Self::_is_tail_call_body(then_branch) && Self::_is_tail_call_body(else_branch),
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .all(|(_, body)| Self::_is_tail_call_body(body))
                    && else_branch
                        .as_ref()
                        .is_some_and(|b| Self::_is_tail_call_body(b))
            }
            HirKind::Begin(exprs) => exprs.last().is_some_and(Self::_is_tail_call_body),
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                Self::_is_tail_call_body(body)
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .all(|(_, _, body)| Self::_is_tail_call_body(body)),
            _ => false,
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
mod intrinsic;
mod walkrest;
