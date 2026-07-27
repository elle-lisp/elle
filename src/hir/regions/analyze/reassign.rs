//! The mutable-reassign 1-slot-container gate.
//!
//! A reassigned mutable binding (top-level file-letrec or fn-local) is modeled
//! as a 1-slot container rather than given a static last-use `decref_point`,
//! which would mis-target whatever the slot holds at that program point. See
//! docs/impl/region/bindings.md "Reassigned mutable bindings are 1-slot
//! containers". Extracted verbatim from the phase that ran inline in
//! `analyze_regions_with`; the long block comments there explain the WHY of
//! each gate condition.

// `super` is `hir::regions::analyze`; `super::super` reaches the sibling
// `hir::regions` items (`RegionHolders`, `RegionInfo`, `Binding`, …) the
// original block saw through `use super::*` at the analyze root.
use super::super::holders::RegionHolders;
use super::super::*;
use crate::hir::defuse::DefUseBuilder;
use crate::hir::region::CellContainer;

/// Apply the 1-slot-container model for reassigned mutable bindings, recording
/// drop-on-overwrite / donation sites and decref suppressions into `info`.
pub(super) fn apply_reassign_containers(
    info: &mut RegionInfo,
    arena: &BindingArena,
    du: &DefUseBuilder,
    inference_binding_regions: &HashMap<Binding, Vec<Region>>,
    top_level_reassigns: &HashMap<Binding, (Vec<HirId>, Vec<Region>)>,
    local_reassigns: &HashMap<Binding, (Vec<HirId>, Vec<Region>)>,
    escape_info: &crate::hir::EscapeInfo,
) {
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
        RegionHolders::from_source_regions(inference_binding_regions, arena, &is_read);
    for (b, (_sites, regions)) in top_level_reassigns.iter().chain(local_reassigns.iter()) {
        if is_read(*b) {
            region_holders.add(*b, arena, regions);
        }
    }
    let sole_held = |b: Binding, r: Region| -> bool { region_holders.sole_held(b, r) };
    // ── Returned-value exclusion ────────────────────────────────────────
    // (docs/impl/region/bindings.md "Reassigned mutable bindings are 1-slot
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
    for (b, (sites, regions)) in top_level_reassigns {
        let init_regions = inference_binding_regions
            .get(b)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        // Backstop (docs/impl/region/bindings.md "a mutated slot is not a
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
        // (docs/impl/region/bindings.md "Reassigned mutable bindings are 1-slot
        // containers"). The fn-local loop deliberately does NOT mark its
        // sites here (its assign-value decref is kept, balancing the incref).
        //
        // CALL-RESULT content is excluded from the donation, exactly as in the
        // fn-local branch below. A call result carries a SECOND compile-time name
        // for the same runtime value — the opaque placeholder region the lowerer
        // releases by value through the ANF temp's slot (Rule 2's bound-result
        // shape) — and the suppression below reaches only the value's own source
        // regions, never that placeholder. So the placeholder release still fires
        // and consumes the callee's single returned reference; donating on top of
        // it leaves the cell holding a freed value (`region-hof-tail-return-uaf.lisp`,
        // whose callee returns a frozen array through a `cond` arm). Taking the
        // counted store instead balances: store incref + placeholder release = the
        // cell's one reference, dropped at the next overwrite.
        let donates = !regions.iter().any(|r| info.call_result_regions.contains(r));
        for &s in sites {
            info.drop_on_overwrite_sites.insert(s);
            if donates {
                info.donated_overwrite_sites.insert(s);
            }
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
    // Same 1-slot-container model as the top-level loop above — the cell takes a
    // COUNTED reference via `lower_assign`'s incref-on-store, released by
    // drop-on-overwrite for each displaced prior. ONE difference: a fn-local
    // cell's final content is NOT a program-lifetime root (a module-scope cell's
    // is, freed by the file-letrec frame teardown), so the cell needs a second
    // release channel of its own — the CONTENT DROP at the cell's scope demise,
    // recorded in `cell_containers` and emitted by the lowerer at the enclosing
    // scope node's exit. The producer's separate claim on each stored value is
    // dead once the cell holds its own reference, so it is pinned to the store
    // site (`decref::populate_decref_points` reads `cell_containers` for both).
    // Two references, two channels each: no release does double duty, so the
    // accounting holds for a cell written once and for one re-minted every
    // iteration of a loop alike.
    //
    // The gate is sole-held (BOTH not-returned and returned — see the split
    // below and docs/impl/region/bindings.md "Reassigned mutable bindings are
    // 1-slot containers" § "Returned fn-local reassigned mutables"). Distinct
    // mechanism: a `@`-mutable PARAMETER (a captured cell the callee owns)
    // reassigned then moved into a tail call is released by the callee's own cell
    // `DecrefCellRegion`, and the tail move's borrowed-arg retain must order ahead
    // of that release's cascade — enforced in `lower_call` (pinned by
    // region-mutable-reassign-param.lisp), not by this gate.
    for (b, (sites, regions)) in local_reassigns {
        // Record the binding so the lowerer can refuse a value-route decref +
        // nil-stamp that names this binding's stack slot. `allocate_slot` gives a
        // fn-local reassigned mutable its own never-reused slot that holds a live
        // value across its whole scope; a spurious immediate-valued assign region
        // (`(assign ii (%add ii 1))`) kept by the branch below would otherwise
        // nil-stamp that slot mid-loop and zero the counter
        // (region-capture-cell-loop-uaf.lisp under --wasm=full).
        info.reassigned_local_bindings.insert(*b);
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

        if sole {
            if !returned {
                // Not returned (and sole-held): the cell's content dies at the
                // overwrite (priors) and at the cell's scope demise (the final
                // value). The first overwrite is the init value's owning demise,
                // so drop-on-overwrite is its release channel too.
                for &s in sites {
                    info.drop_on_overwrite_sites.insert(s);
                }
                // The demise is seeded with the last store — the earliest point
                // that is after every write — and `decref::populate_decref_points`
                // moves it out to the cell's last read and past any loop the
                // cell is carried across, both of which need the structural
                // order this pass runs before.
                if let Some(&seed) = sites.last() {
                    info.cell_containers
                        .insert(*b, CellContainer::new(sites.clone(), regions.clone(), seed));
                }
            }
            // KEEP the assign-value regions' (`regions`) decrefs and suppress
            // every OTHER region the binding may hold (`binding_regs \ regions`
            // — the init region, and, for a binding accumulated in a LOOP, the
            // loop-carried binding region that aliases whatever value the slot
            // currently holds).
            //
            // Not returned: the kept assign-value decref is the PRODUCER's
            // release of each stored value (pinned to the store site, where the
            // cell's counted reference takes over); the cell's own reference is
            // released by drop-on-overwrite and the content drop above. The
            // init value is donated — it is stored uncounted at the define, so
            // suppressing its ordinary decref leaves drop-on-overwrite (or the
            // content drop, if it is never displaced) as its one release.
            //
            // Returned: the binding's value is minted for the caller at the
            // `Return` (`lower_return`'s `IncrefValueRegion`). A loop over the
            // cell gives the binding its OWN loop-carried region (the slot that
            // carries the accumulator across the back-edge) that aliases the SAME
            // runtime value as the reaching assign-value region — so leaving the
            // unsuppressed baseline emits TWO value-route decrefs of that one
            // value (binding-region slot AND assign-value temp) at the Return.
            // The callee owns exactly one reference (the value's birth); the
            // second decref frees the caller's minted reference before the
            // caller's read (the loop-reassigned-return double-free —
            // `region_capture_cell_string_accum_uaf`). Suppressing the
            // binding's own region keeps the single assign-value decref (the
            // callee's one release) and lets the mint carry ownership to the
            // caller. A single-assign returned cell coalesces its binding and
            // assign-value regions (`binding_regs == regions`), so this
            // suppresses nothing there — the mint-plus-lone-decref baseline the
            // scheduler-park guard depends on
            // (`region-reassign-return-park-uaf.lisp`) is untouched.
            for &r in binding_regs {
                if !regions.contains(&r) {
                    info.suppressed_decref_regions.insert(r);
                }
            }
        }
        // else (not sole-held): leave the unsuppressed baseline.
    }
}
