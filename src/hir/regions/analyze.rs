use super::holders::RegionHolders;
use super::letrec::classify_letrec_callees;
use super::*;

// ── Public API ─────────��─────────────────────────────────────────

/// Run region inference on a functionalized HIR tree.
pub fn analyze_regions(hir: &Hir, arena: &BindingArena) -> RegionInfo {
    analyze_regions_with(hir, arena, CallClassification::default())
}

/// Run region inference with call classification data.
pub fn analyze_regions_with(
    hir: &Hir,
    arena: &BindingArena,
    mut call_class: CallClassification,
) -> RegionInfo {
    // Pre-pass: classify letrec-bound lambdas
    let user_imm = classify_letrec_callees(hir, arena, &call_class);
    call_class.user_immediates = user_imm;

    // Authoritative escape facts over the canonical HIR. The reassign
    // 1-slot-container gate below reads this (its **return facet**,
    // `binding_escapes_via_return`) for the gate's not-returned check, instead of
    // recomputing escape from region signals — the one escape analysis every
    // consumer reads. Computed here, before `call_class` is moved into the
    // inference; `analyze_escape` needs the declared native effects
    // (`call_class.effects`) for its store facet, already populated.
    let escape_info = crate::hir::analyze_escape(hir, arena, &call_class);

    // The transferred-returned-subtree cut reads the call classification AFTER
    // the walk consumes it — the declared effects gate its consumer sites (an
    // `Immediate`-native read is harmless) and the fiber symbols name its fiber
    // face. Snapshot it here, before the move into the inference.
    let transfer_call_class = call_class.clone();

    let mut ri = RegionInference::new(arena, call_class);
    // Synthetic program-root region. No Region(0) sentinel — the
    // tree uses Option<Region> for roots, so every region is real.
    let root = ri.tree.fresh_root(&mut ri.next_region);
    ri.current_region = root;
    // The entry function returns the top-level expression's value, so the program
    // tail is part of the return frontier — but that is escape's judgment
    // (`analyze_escape` seeds the top-level tail; `regions::escape` projects it),
    // not a fact the solver records here.
    ri.walk(hir);
    // Capture binding_regions before build_info consumes ri — used
    // below to extend decref_point through binding chains.
    let inference_binding_regions = std::mem::take(&mut ri.binding_regions);
    let return_sites = std::mem::take(&mut ri.return_sites);
    let destructure_sites = std::mem::take(&mut ri.destructure_sites);
    let top_level_reassigns = std::mem::take(&mut ri.top_level_reassigns);
    let local_reassigns = std::mem::take(&mut ri.local_reassigns);
    let captured_reassigns = std::mem::take(&mut ri.captured_reassigns);
    let mut info = ri.build_info();
    // Mirror to the public surface so tests and downstream consumers can
    // inspect which source regions each binding may point into without
    // re-running the inference. Single owner; clone is cheap relative
    // to the cost of re-doing the walk.
    info.binding_source_regions = inference_binding_regions.clone();
    info.captured_reassigned_bindings = captured_reassigns;

    // Populate `region_data.decref_point` from per-HirId last-use analysis.
    // For each region r, `decref_point` is the maximum `last_use[alloc_id]`
    // over all allocation sites that resolved to r. Under unique-per-alloc each
    // region has exactly one contributing alloc_id; the max is kept so the
    // result stays correct should any region ever gather more than one.
    let mut du = DefUseBuilder::new();
    du.walk(hir);

    // ── Mutable-reassign: the cell as a 1-slot container ───────────────────
    // A reassigned top-level (file-letrec) mutable binding holds different
    // values over time; no single static program point names "the value's last
    // use", so a static last-use `decref_point` against the cell is a category
    // error (it mis-targets whatever the slot holds at that point — the read
    // UAF). Model the cell as a 1-slot mutable container instead (docs/impl/region-bindings.md
    // Rule 5): `lower_assign` increfs the new content on store and decrefs the
    // displaced content on overwrite, so each value's region reaches 0 exactly
    // when the cell stops holding it — the next overwrite, or, for the final
    // never-overwritten value, frame teardown.
    //
    // For that to balance, the compiler's own decrefs for the cell's values must
    // be suppressed: each value already carries a static `DecrefRegion` (its ANF
    // `(let [_t v] _t)` scope region) as its single owning demise, and the cell's
    // reference is the store-incref. So here we mark each assign site for
    // drop-on-overwrite and suppress the value-based decref of every region the
    // cell holds (init + each assign value); see the loop body.
    //
    // Sound only when the binding's value regions are SOLE-HELD (no live alias
    // a drop-on-overwrite could free out from under); aliased cases fall back
    // to the existing (over-keeping, UAF-safe) path.
    {
        // A holder is a real alias only if it is a USER binding that is READ:
        // exclude the write-only `__file_expr_N` statement wrapper an assign result
        // flows into (never read), and — via the shared index — the synthetic ANF
        // producer temp `(let [_t e] _t)` (read once, same value flow). Otherwise
        // every reassigned binding looks aliased and the fix never fires. The
        // eligibility filter here is "is read" (`du.uses` non-empty); `RegionHolders`
        // applies the universal synthetic exclusion on top, so the admitted set is
        // exactly the old `counts_as_alias` (read AND non-synthetic).
        let is_read = |b: Binding| -> bool { du.uses.get(&b).is_some_and(|u| !u.is_empty()) };
        let mut region_holders =
            RegionHolders::from_source_regions(&inference_binding_regions, arena, &is_read);
        for (b, (_sites, regions)) in top_level_reassigns.iter().chain(local_reassigns.iter()) {
            if is_read(*b) {
                region_holders.add(*b, arena, regions);
            }
        }
        let sole_held = |b: Binding, r: Region| -> bool { region_holders.sole_held(b, r) };
        // ── Returned-value exclusion ────────────────────────────────────────
        // (docs/impl/region-bindings.md "Reassigned mutable bindings are 1-slot
        // containers".) The container model claims each value region's single
        // compiler-owned reference for the cell (released by drop-on-overwrite
        // or frame/scope teardown) and suppresses the region's ordinary decref.
        // A value that ALSO flows to a function's tail/return is claimed a SECOND
        // time by the return's `IncrefValueRegion` (the mint-at-return
        // convention) — two static claims on one cell, so the gate must refuse
        // and fall back to the unsuppressed baseline (over-keeping, never
        // mis-freeing). The "is this cell's value returned" question is answered
        // per-binding by `EscapeInfo`'s return facet (`binding_escapes_via_return`,
        // below), not by projecting a region set.
        //
        // Deliberately NOT refused — runtime-counted escapes are compatible
        // with the model and must keep the gate (the boundary is pinned by
        // `reassign_gate_keeps_*` tests; refusing them regresses the
        // mutable-reassign pins straight back to UAFs):
        //   - mutable-container stores (push/put funnels incref at runtime),
        //   - capture into a closure env (alloc-scan incref + free cascade),
        //   - opaque-call arg cliques (mutual may-store edges; a real store
        //     increfs at runtime, and the edge's compile-time IncrefRegion is
        //     balanced by the target's free-time cascade),
        //   - value-succession into the binding's own next value
        //     (`(assign acc (pair i acc))` — alloc-scan counted).
        // Like sole_held, the check is per-binding, all-or-nothing.
        for (b, (sites, regions)) in &top_level_reassigns {
            let init_regions = inference_binding_regions
                .get(b)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            // Backstop (docs/impl/region-bindings.md "a mutated slot is not a
            // release route"), recorded UNCONDITIONALLY — before the
            // sole/returned gate. A top-level (file-letrec) reassigned binding's
            // slot is overwritten over time, so a value-routed release
            // (`LoadLocal slot` + `DecrefValueRegion`) of ANY region it holds —
            // init OR assign value — at that region's `decref_point` loads
            // whatever the slot holds THEN (a later, live value) and frees it,
            // not the region intended (the no-alias corruption UAF,
            // region-mutable-reassign-flow facet 3: a deref-cell read is solved
            // to the cell's init region, pushing the init's decref to the read's
            // last use and routing it through the now-reassigned cell slot). When
            // the gate SUCCEEDS these are already in `suppressed_decref_regions`;
            // when it FAILS the lowerer skips the value route for any region here.
            // The final never-overwritten value is freed by file-letrec frame
            // teardown (its region lives in the frame region, cascade-freed), not
            // by a slot route, so skipping ALL of them only over-keeps until
            // teardown — never a leak, never a mis-free. (Fn-local reassigns are
            // NOT recorded: their final value's release IS a legitimate
            // scope-exit slot route, and the scope-based solver shares regions, so
            // skipping there leaks an aliased value — region-tailcall-arg-transfer.)
            for &r in init_regions.iter().chain(regions.iter()) {
                info.mutated_binding_value_regions.insert(r);
            }
            // **Not-returned check reads `EscapeInfo`** (the one authoritative
            // escape analysis). The gate refuses the container model for a *returned*
            // value (the return transfers the value's reference to the caller, which
            // the cell also claims — two static owners) but keeps it for a value that
            // merely stores into a container or is captured (runtime-counted). That
            // is exactly the *return facet*: `binding_escapes_via_return`.
            //
            // Read per-binding (atom-level), NOT by projecting a returned-region set
            // onto the cell's regions — `binding_source_regions` is "where the value
            // points", not "where it lives", so that projection is unsound. Where the
            // return facet is precise about a cell that merely *points* at a returned
            // region without itself flowing to a tail, the value is genuinely not
            // returned, so applying the model is correct; and such shapes are
            // independently sole-held-refused (the "refused twice over" invariant
            // below), so the gate *outcome* is unchanged.
            //
            // Guarded by "the cell carries a heap region": the return facet is
            // value-flow, so an immediate-valued cell read in tail position is
            // "returned", but it carries no reference to transfer — the region model
            // never refused on one, and there is no decref to suppress regardless.
            let has_heap_region = !init_regions.is_empty() || !regions.is_empty();
            let returned = has_heap_region && escape_info.binding_escapes_via_return(*b);
            let all_sole = !returned
                && init_regions
                    .iter()
                    .chain(regions.iter())
                    .all(|&r| sole_held(*b, r));
            if !all_sole {
                continue;
            }
            // Module-scope container: the producer's reference is donated to the
            // cell (its ordinary decref is suppressed below), so the lowerer's
            // drop-on-overwrite is its sole release and NO incref-on-store is added.
            // `donated_overwrite_sites` carries that to `lower_assign` — without it
            // an unbalanced incref holds every displaced prior to teardown
            // (docs/impl/region-bindings.md "Reassigned mutable bindings are 1-slot
            // containers"). The fn-local loop deliberately does NOT mark its
            // sites here (its assign-value decref is kept, balancing the incref).
            for &s in sites {
                info.drop_on_overwrite_sites.insert(s);
                info.donated_overwrite_sites.insert(s);
            }
            // Suppress the compiler's ordinary decrefs for BOTH the init region
            // and every assign-value region. Each of those values ALSO carries a
            // static `DecrefRegion` (its `(let [_t v] _t)` ANF scope region) that
            // is its single owning demise; the value-based `DecrefValueRegion`
            // here would be a SECOND decref of the same region (the read-time
            // double-free witnessed in the rc trace). The cell's own reference is
            // supplied by `lower_assign`'s incref-on-store and released by
            // drop-on-overwrite (priors) or frame teardown (final value).
            for &r in init_regions.iter().chain(regions.iter()) {
                info.suppressed_decref_regions.insert(r);
            }
        }

        // ── Fn-local (in-lambda) reassigned mutables ───────────────────────────
        // Same 1-slot-container model as the top-level loop above — the cell takes
        // a counted reference via `lower_assign`'s drop-on-overwrite (incref the
        // new content on store, decref the displaced prior on overwrite). ONE
        // difference: a fn-local cell's final value is NOT a program-lifetime root
        // (top-level cells are); the binding's scope exits and its `decref_point`
        // frees whatever the cell last held. So we KEEP the assign-value regions'
        // decrefs — they ARE the cell's scope-exit demise — and suppress ONLY the
        // init region's decref. (Top-level suppresses the assign-value regions too
        // because there the final value lives forever; doing that here would drop
        // the cell's scope-exit decref and leak the final value.)
        //
        // The defect this fixes: without the incref-on-store the cell slot holds an
        // UNCOUNTED reference (plain `StoreLocal`) yet still receives that
        // scope-exit `DecrefValueRegion` — one decref too many for the final value,
        // whose producer temp already owns the lone reference (rc=1). The second
        // decref frees an already-freed region and, once its physical id recycles,
        // double-frees a live one (the `regionstore.rs` phantom-region panic;
        // `fn/cfg … :mermaid`). The init value is released by the first assign's
        // drop-on-overwrite, so its ordinary decref (suppressed here) is the
        // duplicate. SOLE-HELD gates this, as for the top-level path.
        //
        // The gate is sole-held AND not-returned, as for the top-level path
        // (`returned_regions` above; docs/impl/region-bindings.md "Reassigned mutable
        // bindings are 1-slot containers"). Distinct mechanism: a `@`-mutable PARAMETER
        // (a captured cell the callee owns) reassigned then moved into a tail call is
        // released by the callee's own cell `DecrefCellRegion`, and the tail move's
        // borrowed-arg retain must order ahead of that release's cascade — enforced in
        // `lower_call` (pinned by region-mutable-reassign-param.lisp), not by this gate.
        for (b, (sites, regions)) in &local_reassigns {
            let binding_regs = inference_binding_regions
                .get(b)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let sole = binding_regs.iter().all(|&r| sole_held(*b, r));
            // Does the binding's value escape via the function's tail (read in
            // return position)? `EscapeInfo`'s return facet
            // (`binding_escapes_via_return`), read per-binding — see the top-level
            // gate's note on why this is atom-level, not a region projection, and
            // why the recorded class-3 divergences leave the gate outcome unchanged.
            // Guarded by "carries a heap region" (immediate cells carry no reference).
            let returned = !binding_regs.is_empty() && escape_info.binding_escapes_via_return(*b);

            if sole && !returned {
                // Not returned (and sole-held): the cell's final value dies at
                // scope exit. KEEP the assign-value regions' decrefs (that IS the
                // scope-exit demise) and suppress ONLY the init region's decref —
                // the first overwrite is the init value's owning demise. A
                // *returned* sole-held cell leaves the unsuppressed baseline: under
                // the mint-at-return convention the return's `IncrefValueRegion`
                // hands the caller its own reference, so the callee's ordinary
                // decref of the cell's value is correct (no double-release).
                for &s in sites {
                    info.drop_on_overwrite_sites.insert(s);
                }
                for &r in binding_regs {
                    if !regions.contains(&r) {
                        info.suppressed_decref_regions.insert(r);
                    }
                }
            }
            // else (returned, or not sole-held): leave the unsuppressed baseline.
        }
    }

    // Explicit structural execution-order index. `decref_point` selection
    // below compares these indices, never `HirId` magnitude (which ANF
    // makes meaningless — see `compute_order`).
    let order = compute_order(hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let last_use_info = compute_last_use(hir, &du.uses, &order);
    let last_use = &last_use_info.per_node;
    for (alloc_id, &region) in &info.alloc_region {
        let lu = last_use.get(alloc_id).copied().unwrap_or(*alloc_id);
        info.region_data
            .entry(region)
            .and_modify(|d| {
                if ord(lu) > ord(d.decref_point) {
                    d.decref_point = lu;
                }
            })
            .or_insert(RegionData { decref_point: lu });
    }
    // Pre-allocated capture cells (one region per cell, keyed by the Begin's
    // HirId in `begin_cell_regions` — not in `alloc_region`, which holds one
    // region per HirId). Base each cell region's `decref_point` on the Begin's
    // last use, exactly as an `alloc_region` entry at the Begin would get;
    // the binding-chain extension below then lifts it over the binding's own
    // uses. The base matters for a captured-but-never-used binding: without
    // it the region has no `region_data` entry, so no `DecrefRegion` is ever
    // emitted and the cell's initial reference leaks (Rule 8).
    for (begin_id, cells) in &info.begin_cell_regions {
        let lu = last_use.get(begin_id).copied().unwrap_or(*begin_id);
        for &(_b, region) in cells {
            info.region_data
                .entry(region)
                .and_modify(|d| {
                    if ord(lu) > ord(d.decref_point) {
                        d.decref_point = lu;
                    }
                })
                .or_insert(RegionData { decref_point: lu });
        }
    }

    // Extend decref_point through binding chains: when a binding b holds a
    // value whose region r is somewhere else (e.g., `(let [result (let
    // [f ...] (array ok val))])`, `result`'s value lives in `array`'s
    // region — bound through the inner `let`'s body), the alloc-id
    // lookup above doesn't see r through b's uses because compute_last_use
    // only extends last_use for the binding's init HirId, not the
    // nested allocation's HirId. Without this extension r is freed at
    // the inner expression's tail, before b is ever read.
    //
    // For each binding b, find the max last_use among b's uses, and
    // extend region_data[r].decref_point for every region r in the
    // inference's binding_regions[b].
    let binding_uses = &du.uses;
    for (b, regions) in &inference_binding_regions {
        if regions.is_empty() {
            continue;
        }
        let mut max_use = binding_uses
            .get(b)
            .into_iter()
            .flat_map(|v| v.iter())
            .map(|use_id| last_use.get(use_id).copied().unwrap_or(*use_id))
            .max_by_key(|id| ord(*id));
        // A binding captured by a lambda built inside a loop (while bound
        // outside it) must outlive the loop: its capture-use's last_use sits
        // inside the body, but the region demise must be hoisted to the loop
        // node, else it fires per iteration and frees the binding mid-loop
        // (region-loop-capture-squelch.lisp / supervisor-style UAF).
        if let Some(&ext) = last_use_info.capture_loop_ext.get(b) {
            if max_use.is_none_or(|cur| ord(ext) > ord(cur)) {
                max_use = Some(ext);
            }
        }
        if let Some(lu) = max_use {
            for &r in regions {
                info.region_data
                    .entry(r)
                    .and_modify(|d| {
                        if ord(lu) > ord(d.decref_point) {
                            d.decref_point = lu;
                        }
                    })
                    .or_insert(RegionData { decref_point: lu });
                // Record the binding-resolved (tight) last-use per region, for the
                // ownership lifetime obligation. Unlike
                // `region_data` above this is NOT max'd with the structural
                // alloc-site last-use, so a captured value's lambda-as-`let`-init
                // over-estimate — which the grow-only last-use fixpoint leaves
                // locked one step past the closure's last call, and the alloc-loop
                // then writes into `region_data` — does not leak in. `lu` already
                // reads the FINAL resolved `last_use` (the closure binding's last
                // use, not the lambda's structural position), so it is the tight
                // value the obligation needs. Max-by-order across a region's holder
                // bindings: a shared region must outlive every holder.
                info.binding_last_use
                    .entry(r)
                    .and_modify(|cur| {
                        if ord(lu) > ord(*cur) {
                            *cur = lu;
                        }
                    })
                    .or_insert(lu);
            }
        }
    }

    // ── Env-cell release: hoist past enclosing loops (once per activation) ──
    // An env cell — a captured local's `populate_env` cell or a captured param's
    // cell, marked in `cell_release_regions` — is minted EXACTLY ONCE per
    // activation (populate_env runs once when the activation is created), so its
    // `DecrefCellRegion` must fire exactly once per activation. The binding-chain
    // extension above can leave a cell-release region's `decref_point` at an
    // in-loop capture-use (the only use of a `@`-mutable local defined and
    // captured inside the loop), where it fires every iteration. For a closure
    // called in place and dying within the iteration, each iteration nets the box
    // region -1 (capture-incref +1, closure free-cascade -1, DecrefCellRegion -1)
    // — so the once-allocated box is freed at the end of iteration 1 and the next
    // iteration reads the recycled cell (the env-cell-in-loop UAF;
    // tests/elle/region-capture-cell-loop-uaf.lisp, cap2.lisp). Hoist each
    // cell-release region's `decref_point` to the OUTERMOST enclosing While/Loop,
    // which the lowerer emits AFTER the loop (the proven post-loop emission point
    // the bound-outside `capture_loop_ext` extension already targets) — once per
    // activation, matching the once-per-activation populate_env allocation.
    //
    // Sound for every env cell: the box is never re-allocated per iteration, so a
    // once-per-activation release can only over-keep (until the loop exits), never
    // mis-free. It composes with the closure-capture incref — an escaping
    // closure's reference keeps the box alive past the post-loop release, so the
    // box dies with the last surviving closure. This is the env-cell exception to
    // the value-binding rule the `capture_loop_ext` "bound outside" guard
    // enforces: a value bound INSIDE a loop is re-allocated per iteration and its
    // release must stay per-iteration, but an env cell's allocation is
    // loop-independent. See docs/impl/region-bindings.md "Env cells in loops:
    // release once per activation, not per iteration".
    if !info.cell_release_regions.is_empty() {
        let low = compute_subtree_low(hir, &order);
        let mut iter_scopes: Vec<(HirId, u32, u32)> = Vec::new();
        collect_iter_scopes(hir, &order, &low, &mut iter_scopes);
        if !iter_scopes.is_empty() {
            // Snapshot the cell regions first — the loop mutates `region_data`.
            let cell_regions: Vec<Region> = info.cell_release_regions.iter().copied().collect();
            for r in cell_regions {
                let Some(dp) = info.region_data.get(&r).map(|d| d.decref_point) else {
                    continue;
                };
                let dord = ord(dp);
                // Outermost enclosing loop = the iter-scope whose post-order
                // subtree interval `[low, order]` contains `dp` with the largest
                // `order` (an ancestor has the largest post-order index). `None`
                // if `dp` is in no loop — no hoist needed.
                let outermost = iter_scopes
                    .iter()
                    .filter(|&&(_, lo, hi)| lo <= dord && dord <= hi)
                    .max_by_key(|&&(_, _, hi)| hi)
                    .map(|&(id, _, _)| id);
                if let Some(loop_id) = outermost {
                    if ord(loop_id) > dord {
                        info.region_data.get_mut(&r).unwrap().decref_point = loop_id;
                    }
                }
            }
        }
    }

    // Extend each returned value's region `decref_point` to its `Return`
    // node. The lowerer emits the node's `IncrefValueRegion` before the
    // node's own `emit_decrefs_for`; pinning `decref_point` here guarantees a
    // freshly-allocated result region's `DecrefRegion` fires *after* the
    // retain (so the result survives its callee-side release and is
    // handed back with one owning reference). For a pass-through arg the
    // region is the callee's phantom scope (guard-suppressed) and this
    // is inert.
    for (return_id, regions) in &return_sites {
        let lu = *return_id;
        for &r in regions {
            info.region_data
                .entry(r)
                .and_modify(|d| {
                    if ord(lu) > ord(d.decref_point) {
                        d.decref_point = lu;
                    }
                })
                .or_insert(RegionData { decref_point: lu });
        }
    }

    // Extend each destructured value's regions' `decref_point` to its
    // `Destructure` node. A Destructure CONSUMES its value: the field
    // extraction (`StructGetOrNil` and friends) reads the value AFTER the
    // value expression's own last read, so a release anchored at the inner
    // read frees the source under the extraction. Bites exactly when no
    // destructured binding is used afterwards — the `&named`-param
    // prologue with unused params (docs/impl/region-rules.md Rule 4;
    // tests/elle/region-named-param-uaf.lisp, the lib/http2 import segv).
    for (destructure_id, regions) in &destructure_sites {
        let lu = *destructure_id;
        for &r in regions {
            info.region_data
                .entry(r)
                .and_modify(|d| {
                    if ord(lu) > ord(d.decref_point) {
                        d.decref_point = lu;
                    }
                })
                .or_insert(RegionData { decref_point: lu });
        }
    }

    // Per-path branch compensation (`regions::compensate`): a region whose single
    // `decref_point` sits inside a conditional arm is freed there on the used
    // path but leaks on the sibling arms; add a compensating release at each dead
    // sibling arm's head. Reads the FINAL `region_data` (the decref_point post-passes
    // above) and the exclusion sets, so it runs after them. Independent of the merge
    // seed below (a merge child is excluded), but placed before it for locality.
    let branch_comp = super::compensate::compute_branch_compensation(
        hir,
        &info,
        &escape_info,
        &du,
        arena,
        &order,
        last_use,
    );
    info.branch_compensation = branch_comp.head;
    info.branch_arm_decrefs = branch_comp.tail;

    // The builder-idiom merge seed (docs/impl/region-model.md § Merging). Runs
    // LAST: its coincident-decref_point gate reads the final `region_data`, so it
    // must follow every decref_point post-pass above. The lowerer consumes the
    // resulting `merged_parent` forest through `static_slot`'s `merged_root`
    // canonicalization (one slot per merge tree); an empty forest leaves it the
    // identity, i.e. the unmerged baseline.
    info.merged_parent = super::merge::compute_merges(hir, arena, &info, &escape_info, &order);

    // The letrec closure-cycle merge (docs/impl/region-model.md § The letrec
    // closure-cycle merge): a self/mutual
    // recursive closure SCC and its prebound capture cells collapse onto one arena,
    // extending the same `merged_parent` forest and riding the same `merged_root`
    // canonicalization as the builder-idiom seed. Unconditional (not flag-gated), so it
    // lands on every tier. The single `DecrefRegion` fires at the root region's
    // `decref_point`, which is set here to the cycle's binding scope — the `letrec`
    // that prebinds the members' capture cells (region-model.md § The lifetime
    // obligation the root carries). Its scope-exit post-dominates every direct use of
    // the members (they are bound there), while a foreign capture of a member is
    // RC-counted and outlives the single decref. Runs after the builder seed (it
    // shares the map) and before the ownership pass (so `is_merged` excludes these
    // members).
    for cm in super::merge::compute_closure_cycle_merges(hir, arena, &info, &escape_info, &order) {
        for &m in &cm.members {
            // The member set (roots included) feeds `tail_callee_adopts`' refusal
            // and the letrec-body stranded-cycle marking: the merged arena has
            // exactly one release channel, so no other tail call may adopt it.
            info.closure_cycle_members.insert(m);
            if m != cm.root {
                info.merged_parent.insert(m, cm.root);
            }
        }
        // A NON-member body tail (a native `%add`, a redefined `+`, a foreign `g`)
        // strands the binding-scope drop past the frame-replacing `TailCall`; the
        // lowerer keys `adopt_region_slot = static_slot(root)` at each such site so a
        // closure callee's frame replacement is balanced by the activation-completion
        // adopt (region-model.md § The letrec closure-cycle merge). A member tail
        // keeps its own `stranded_cycle_bindings` channel and is not recorded here.
        for &site in &cm.tail_adopt_sites {
            info.cycle_tail_adopt.insert(site, cm.root);
        }
        info.region_data
            .entry(cm.root)
            .and_modify(|d| d.decref_point = cm.drop_site)
            .or_insert(RegionData {
                decref_point: cm.drop_site,
            });
    }

    // Ownership forest (docs/impl/region-model.md § "Adoption and subtree drop").
    // Classify externally-unique Owned subtrees and record their interior
    // containment edges as `AdoptRegion` sites (with the lifetime obligation and
    // merge-overlap filters applied). Runs LAST, after the final `region_data`
    // (its lifetime filter reads `decref_point`) and after the merge seed (so it
    // can exclude merge participants). This is unconditional — the ownership
    // forest is how the language runs, not an opt-in dialect (§ "One semantics,
    // every backend"); a subtree the inference cannot prove externally unique
    // simply stays Shared (the always-legal per-region-RC baseline), so no adopt
    // edge is emitted for it and its emission is the RC baseline by construction.
    let mut adopt = super::ownership::compute_adopt_edges(hir, &info, &escape_info, arena, &order);
    // The transferred-returned-subtree cut (docs/impl/region-model.md
    // § "Owner nodes" — "The transferred returned subtree"): a producer's
    // externally-unique returned cycle is owned by its CONSUMING activation. Its
    // interior owner edges merge into the ordinary adopt maps here — BEFORE the
    // capture-suppression loop below, so a transfer capture member rides the same
    // suppress ⊆ adopt contract — and each consumer site's call-result region lands
    // in `transfer_adopt_regions`, whose release the lowerer replaces with
    // `AdoptIntoActivation` (regiondecref.rs). Disjoint from the maps' existing
    // entries by construction: a subtree containing the returned root is refused by
    // the seed-poisoned subtree walk, and a transfer member reached from any outside
    // container fails external uniqueness.
    let transfer = super::ownership::compute_transfer_adopts(
        hir,
        &info,
        &escape_info,
        arena,
        &transfer_call_class,
        &order,
    );
    for (site, edges) in transfer.store {
        adopt.store.entry(site).or_default().extend(edges);
    }
    for (site, edges) in transfer.capture {
        adopt.capture.entry(site).or_default().extend(edges);
    }
    info.transfer_adopt_regions = transfer.result_regions;
    // A capture-adopted member is reclaimed solely by its closure's subtree drop, so
    // suppress its own compiler decref. This is load-bearing, unlike a STORE-adopted
    // member: the lifetime obligation bounds a store member's `decref_point` at or
    // below the root's drop (its decref hits the still-frozen region — a no-op), but a
    // captured member's `decref_point` is the over-extended structural position one
    // step past the closure (the over-keep the TIGHT obligation admits past), so its
    // unsuppressed decref would fire AFTER the subtree drop freed it — a direct decref
    // of an absent region, tripping the `regionstore` phantom/double-free assert.
    // Collected from `adopt.capture` before the maps move into `info`.
    for edges in adopt.capture.values() {
        for &(member, _closure) in edges {
            info.suppressed_decref_regions.insert(member);
        }
    }
    info.owned_adopt_edges = adopt.store;
    info.capture_adopt_edges = adopt.capture;
    // The cell⊇content adopts are emitted at the cell's own store site (keyed by binding),
    // as `AdoptCellRegion(cell, content)`. The content is store-adopted (its own decref is
    // a frozen no-op under the Owned region), so it is NOT suppressed here; the cell region
    // — a capture-adopted member of `adopt.capture` — already was, above.
    info.cell_content_adopt_bindings = adopt.cell_content.iter().copied().collect();

    // Co-owned-cycle cut: a rootless mutual reference cycle is reclaimed
    // symmetrically as one `FreeRegionGroup` at its collective last use, disjoint
    // from the container-rooted adopt subtrees above. `owned_group_members` is the
    // flat union, the O(1) decref-skip set the lowerer consults.
    let groups =
        super::ownership::compute_owned_region_groups(hir, &info, &escape_info, arena, &order);
    info.owned_group_members = groups.values().flatten().copied().collect();
    info.owned_region_groups = groups;

    // The activation-owner cut: a capture-back-edge SCC — a container captured by a
    // closure it holds, the cycle no region root can own — is adopted into the
    // executing activation's owner node and freed by its completion release
    // (docs/impl/region-model.md § "Owner nodes" — "The capture-back-edge SCC").
    // Runs LAST among the ownership passes: its disjointness gate reads the merge,
    // adopt, and group claims above. Each member's own compiler decref is suppressed
    // — the node's release is the members' sole demise (the suppress ⊆ adopt
    // contract) — and every decref-emit site re-checks `suppressed_decref_regions`,
    // so no other release path can reach a member.
    let activation =
        super::ownership::compute_activation_adopts(hir, &info, &escape_info, arena, &order);
    for members in activation.values() {
        for &m in members {
            info.suppressed_decref_regions.insert(m);
        }
    }
    info.activation_adopt_sites = activation;

    info
}

/// Collect every iterative-scope node (`While` or `Loop` — `while` lowers to
/// either) with its post-order subtree interval `[low, order]`, so containment
/// of a HirId is an interval test (`low <= ord(x) <= order`; see
/// `compute_subtree_low`). Used by the env-cell release hoist to find the
/// outermost loop enclosing a cell-release region's `decref_point`.
fn collect_iter_scopes(
    hir: &Hir,
    order: &HashMap<HirId, u32>,
    low: &HashMap<HirId, u32>,
    out: &mut Vec<(HirId, u32, u32)>,
) {
    if matches!(&hir.kind, HirKind::While { .. } | HirKind::Loop { .. }) {
        let lo = low.get(&hir.id).copied().unwrap_or(0);
        let hi = order.get(&hir.id).copied().unwrap_or(0);
        out.push((hir.id, lo, hi));
    }
    hir.for_each_child(|c| collect_iter_scopes(c, order, low, out));
}
