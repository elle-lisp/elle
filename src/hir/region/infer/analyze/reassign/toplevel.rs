// audited: 2026-09-22
//! The module-scope half of the 1-slot-container model: the file-letrec cell
//! that ADOPTS its producer's reference instead of counting one.
//!
//! docs/impl/region/bindings.md

use super::super::super::*;
use super::chain::ReassignSites;

/// Apply the model to every module-scope (file-letrec) reassigned binding.
///
/// The cell adopts the producer reference for the init and every stored value
/// alike, so the gate is sole-held over ALL of them and the lowerer emits no
/// incref-on-store; the final, never-overwritten value is released by the
/// file-letrec frame teardown rather than by a content drop of its own.
pub(super) fn apply_module_scope(
    info: &mut RegionInfo,
    top_level_reassigns: &ReassignSites,
    inference_binding_regions: &HashMap<Binding, Vec<Region>>,
    escape_info: &crate::hir::EscapeInfo,
    sole_held: &dyn Fn(Binding, Region) -> bool,
) {
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
    for (b, stores) in top_level_reassigns {
        let regions = stores.value_region_set();
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
        for s in stores.sites() {
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
}
